//! Windows D3D11VA decoder session.

use std::marker::PhantomData;
use std::rc::Rc;

use dvs_gpu::{D3d11DecodedSurfaceRef, DxgiAdapterLuid};
use dvs_media::VideoFrameMetadata;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
use windows::core::Interface;

use crate::error::DecoderError;
use crate::ffmpeg::{
    AvCodecContext, AvFormatContext, AvFrame, AvHwDeviceRef, AvPacket, ReadPacketResult,
    ReceiveResult, SendResult, adapter_luid_from_hw_device, borrow_d3d11_decoder_surface,
    borrow_ffmpeg_d3d11_device, build_external_context_lock, clone_ffmpeg_d3d11_device_context,
    create_d3d11va_device, init_ffmpeg_network, open_d3d11_decoder, read_frame_fields,
};
use crate::metadata::{build_frame_metadata, next_frame_id};

/// FFmpeg D3D11VA decoder session that returns borrowed GPU surfaces.
///
/// Decoder-thread only. Not `Send` or `Sync`.
///
/// Rust drops struct fields in declaration order. The order below ensures the decoded
/// `AVFrame` (and any D3D11 texture it references) is released before the codec context,
/// hardware device, and demuxer are torn down:
/// `current_frame` → `packet` → `codec` → `d3d11_context` → `hw_device` → `format`.
pub struct DecoderSession {
    current_frame: AvFrame,
    packet: AvPacket,
    codec: AvCodecContext,
    /// Immediate context for FFmpeg's D3D11VA device; released before `hw_device`.
    d3d11_context: ID3D11DeviceContext,
    /// Keeps the FFmpeg D3D11VA hardware device alive until after the codec is freed.
    hw_device: AvHwDeviceRef,
    format: AvFormatContext,
    stream_index: i32,
    time_base_num: i32,
    time_base_den: i32,
    next_frame_id: u64,
    demux_eof: bool,
    flush_sent: bool,
    decode_finished: bool,
    adapter_luid: DxgiAdapterLuid,
    _thread_bound: PhantomData<Rc<()>>,
}

/// One decoded D3D11 frame borrowed from the session's current `AVFrame`.
///
/// Dropping this value releases only the Rust surface borrow. The underlying FFmpeg
/// `AVFrame` (and its pooled array slice) remain held in [`DecoderSession::current_frame`]
/// until the next [`DecoderSession::decode_next_d3d11`] call unrefs it.
///
/// Must be dropped before the next [`DecoderSession::decode_next_d3d11`] call.
pub struct DecodedD3d11Frame<'a> {
    metadata: VideoFrameMetadata,
    surface: D3d11DecodedSurfaceRef<'a>,
    _thread_bound: PhantomData<Rc<()>>,
}

/// FFmpeg D3D11VA device handles for interop producer setup.
///
/// `context` is a cloned reference to FFmpeg's `AVD3D11VADeviceContext.device_context`
/// (the same immediate context FFmpeg uses internally). Both remain valid for the
/// lifetime of the decoder session.
pub struct DecoderD3d11Hardware<'a> {
    device: &'a ID3D11Device,
    context: &'a ID3D11DeviceContext,
}

impl<'a> DecoderD3d11Hardware<'a> {
    /// Returns FFmpeg's D3D11VA device used for decoded surfaces.
    pub fn device(&self) -> &ID3D11Device {
        self.device
    }

    /// Returns FFmpeg's immediate `device_context` used for interop copies.
    pub fn context(&self) -> &ID3D11DeviceContext {
        self.context
    }
}

impl<'a> DecodedD3d11Frame<'a> {
    /// Returns the validated frame metadata.
    pub fn metadata(&self) -> VideoFrameMetadata {
        self.metadata
    }

    /// Splits the decoded frame into metadata and the borrowed D3D11 surface reference.
    pub fn into_parts(self) -> (VideoFrameMetadata, D3d11DecodedSurfaceRef<'a>) {
        (self.metadata, self.surface)
    }
}

impl DecoderSession {
    /// Opens a D3D11VA decoder for the best video stream in `path`.
    ///
    /// `required_adapter` must match the wgpu/DX12 adapter LUID selected before FFmpeg
    /// initialization. FFmpeg owns D3D11 device creation.
    pub fn open(
        path: impl AsRef<std::path::Path>,
        required_adapter: DxgiAdapterLuid,
    ) -> Result<Self, DecoderError> {
        init_ffmpeg_network()?;

        let path_ref = path.as_ref();
        if !path_ref.exists() {
            return Err(crate::error::DecoderError::InputPathNotFound {
                path: path_ref.display().to_string(),
            });
        }

        let format = AvFormatContext::open(path_ref)?;
        format.discover_streams()?;
        let stream_index = format.find_best_video_stream()?;
        let (time_base_num, time_base_den) = format.stream_time_base(stream_index)?;
        let codecpar = format.codec_parameters(stream_index)?;

        let hw_device = create_d3d11va_device()?;
        let adapter_luid = adapter_luid_from_hw_device(&hw_device, required_adapter)?;
        let d3d11_context = clone_ffmpeg_d3d11_device_context(&hw_device)?;
        let codec = open_d3d11_decoder(codecpar, &hw_device)?;

        Ok(Self {
            current_frame: AvFrame::new()?,
            packet: AvPacket::new()?,
            codec,
            d3d11_context,
            hw_device,
            format,
            stream_index,
            time_base_num,
            time_base_den,
            next_frame_id: 0,
            demux_eof: false,
            flush_sent: false,
            decode_finished: false,
            adapter_luid,
            _thread_bound: PhantomData,
        })
    }

    /// Returns the validated DXGI adapter LUID for FFmpeg's D3D11VA device.
    pub fn adapter_luid(&self) -> DxgiAdapterLuid {
        self.adapter_luid
    }

    /// Returns borrowed FFmpeg D3D11VA device handles for interop producer setup.
    pub fn d3d11_hardware(&self) -> Result<DecoderD3d11Hardware<'_>, DecoderError> {
        Ok(DecoderD3d11Hardware {
            device: borrow_ffmpeg_d3d11_device(&self.hw_device)?,
            context: &self.d3d11_context,
        })
    }

    /// Returns FFmpeg lock callbacks for external `device_context` use (interop producer).
    ///
    /// The returned lock owns a retained `AVBufferRef` clone so producer-side callback
    /// invocations remain valid even if the [`DecoderSession`] is dropped first.
    pub fn external_context_lock(&self) -> Result<dvs_gpu::D3d11ExternalContextLock, DecoderError> {
        build_external_context_lock(&self.hw_device)
    }

    /// Debug-only: asserts the session context matches FFmpeg `device_context`.
    #[cfg(debug_assertions)]
    pub fn debug_assert_same_ffmpeg_device_context(
        &self,
        context: &ID3D11DeviceContext,
    ) -> Result<(), DecoderError> {
        use crate::ffmpeg::ffmpeg_d3d11_device_context_ptr;

        let ffmpeg_ptr = ffmpeg_d3d11_device_context_ptr(&self.hw_device)?;
        let session_ptr = context.as_raw();
        if !std::ptr::eq(ffmpeg_ptr, session_ptr) {
            return Err(DecoderError::InvalidDecoderState {
                detail: "device_context COM identity mismatch",
            });
        }
        Ok(())
    }

    /// Decodes the next D3D11 frame, draining delayed frames after demux EOF.
    ///
    /// Returns `Ok(None)` only after the decoder flush reaches `AVERROR_EOF`.
    pub fn decode_next_d3d11(&mut self) -> Result<Option<DecodedD3d11Frame<'_>>, DecoderError> {
        if self.decode_finished {
            return Ok(None);
        }

        loop {
            self.current_frame.unref();
            match self.codec.receive_frame(&mut self.current_frame)? {
                ReceiveResult::Frame => {
                    return Ok(Some(self.build_decoded_frame()?));
                }
                ReceiveResult::Eof => {
                    self.decode_finished = true;
                    return Ok(None);
                }
                ReceiveResult::Again => {}
            }

            if self.demux_eof {
                if !self.flush_sent {
                    self.send_flush()?;
                    self.flush_sent = true;
                    continue;
                }
                return Err(DecoderError::InvalidDecoderState {
                    detail: "decoder returned EAGAIN after flush without EOF",
                });
            }

            if !self.read_and_send_next_packet()? {
                self.demux_eof = true;
            }
        }
    }

    fn build_decoded_frame(&mut self) -> Result<DecodedD3d11Frame<'_>, DecoderError> {
        let borrowed = borrow_d3d11_decoder_surface(&self.current_frame)?;
        let fields = read_frame_fields(&self.current_frame)?;
        let frame_id = next_frame_id(self.next_frame_id);
        self.next_frame_id = self.next_frame_id.saturating_add(1);

        let metadata = build_frame_metadata(
            frame_id,
            fields.pts,
            self.time_base_num,
            self.time_base_den,
            borrowed.allocation_width,
            borrowed.allocation_height,
            fields.visible_x,
            fields.visible_y,
            fields.visible_width,
            fields.visible_height,
            fields.color_range,
            fields.colorspace,
            fields.color_primaries,
            fields.color_trc,
        )?;

        let surface = D3d11DecodedSurfaceRef::new(borrowed.texture, borrowed.array_slice)
            .map_err(DecoderError::Gpu)?;

        Ok(DecodedD3d11Frame {
            metadata,
            surface,
            _thread_bound: PhantomData,
        })
    }

    fn send_flush(&self) -> Result<(), DecoderError> {
        match self.codec.send_packet(None)? {
            SendResult::Accepted | SendResult::Again => Ok(()),
        }
    }

    fn read_and_send_next_packet(&mut self) -> Result<bool, DecoderError> {
        loop {
            self.packet.unref();
            match self.format.read_packet(&mut self.packet)? {
                ReadPacketResult::Eof => return Ok(false),
                ReadPacketResult::Packet => {
                    if self.packet.stream_index() != self.stream_index {
                        continue;
                    }

                    match self.codec.send_packet(Some(&self.packet))? {
                        SendResult::Accepted | SendResult::Again => return Ok(true),
                    }
                }
            }
        }
    }
}
