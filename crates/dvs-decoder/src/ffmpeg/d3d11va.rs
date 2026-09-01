//! FFmpeg D3D11VA device creation, adapter validation, and surface borrowing.

use std::ffi::c_void;

use dvs_gpu::{DxgiAdapterLuid, validate_same_adapter};
use windows::Win32::Graphics::Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::core::Interface;

use crate::error::DecoderError;
use crate::ffmpeg::ffi::ffmpeg_err;
use crate::ffmpeg::raii::{AvCodecContext, AvFrame, AvHwDeviceRef};

const AV_PIX_FMT_D3D11: i32 = ffmpeg_sys_next::AVPixelFormat::AV_PIX_FMT_D3D11 as i32;
const AV_PIX_FMT_NONE: ffmpeg_sys_next::AVPixelFormat =
    ffmpeg_sys_next::AVPixelFormat::AV_PIX_FMT_NONE;

/// FFmpeg `AVD3D11VADeviceContext` first field layout (`ID3D11Device *device`).
#[repr(C)]
struct AvD3d11VaDeviceContext {
    device: *mut c_void,
}

/// FFmpeg `get_format` callback that selects `AV_PIX_FMT_D3D11` only when offered.
extern "C" fn d3d11_get_format(
    _avctx: *mut ffmpeg_sys_next::AVCodecContext,
    pix_fmts: *const ffmpeg_sys_next::AVPixelFormat,
) -> ffmpeg_sys_next::AVPixelFormat {
    // SAFETY: FFmpeg calls this callback with a valid null-terminated pixel-format list.
    unsafe {
        if pix_fmts.is_null() {
            return AV_PIX_FMT_NONE;
        }

        let mut index = 0;
        loop {
            let fmt = *pix_fmts.add(index);
            if fmt == AV_PIX_FMT_NONE {
                break;
            }
            if fmt as i32 == AV_PIX_FMT_D3D11 {
                return fmt;
            }
            index += 1;
        }

        AV_PIX_FMT_NONE
    }
}

/// Creates an FFmpeg-owned D3D11VA hardware device context.
pub(crate) fn create_d3d11va_device() -> Result<AvHwDeviceRef, DecoderError> {
    let mut hw_device_ctx: *mut ffmpeg_sys_next::AVBufferRef = std::ptr::null_mut();
    // SAFETY: FFmpeg creates and returns an `AVBufferRef` to a D3D11VA device context.
    let ret = unsafe {
        ffmpeg_sys_next::av_hwdevice_ctx_create(
            &mut hw_device_ctx,
            ffmpeg_sys_next::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ret < 0 {
        return Err(ffmpeg_err(ret));
    }
    if hw_device_ctx.is_null() {
        return Err(DecoderError::D3d11vaUnavailable);
    }
    Ok(AvHwDeviceRef::from_ptr(hw_device_ctx))
}

/// Opens a hardware decoder for the selected stream and attaches D3D11VA.
pub(crate) fn open_d3d11_decoder(
    codecpar: *const ffmpeg_sys_next::AVCodecParameters,
    hw_device: &AvHwDeviceRef,
) -> Result<AvCodecContext, DecoderError> {
    // SAFETY: `codecpar` is a live stream codec parameters object from demux.
    let codec_id = unsafe { (*codecpar).codec_id };
    // SAFETY: `codec_id` is a valid FFmpeg codec identifier from stream parameters.
    let decoder = unsafe { ffmpeg_sys_next::avcodec_find_decoder(codec_id) };
    if decoder.is_null() {
        return Err(DecoderError::InvalidDecoderState {
            detail: "avcodec_find_decoder returned null",
        });
    }

    // SAFETY: `decoder` is a valid codec implementation pointer from FFmpeg.
    let mut codec_ctx = unsafe { ffmpeg_sys_next::avcodec_alloc_context3(decoder) };
    if codec_ctx.is_null() {
        return Err(DecoderError::InvalidDecoderState {
            detail: "avcodec_alloc_context3 failed",
        });
    }

    // SAFETY: `codec_ctx` and `codecpar` are valid FFmpeg objects.
    let ret = unsafe { ffmpeg_sys_next::avcodec_parameters_to_context(codec_ctx, codecpar) };
    if ret < 0 {
        // SAFETY: `codec_ctx` was allocated above and must be freed on failure.
        unsafe {
            ffmpeg_sys_next::avcodec_free_context(&mut codec_ctx);
        }
        return Err(ffmpeg_err(ret));
    }

    // SAFETY: `hw_device` is a live buffer reference to a D3D11VA device context.
    let hw_ref = unsafe { ffmpeg_sys_next::av_buffer_ref(hw_device.as_ptr()) };
    if hw_ref.is_null() {
        // SAFETY: `codec_ctx` was allocated above and must be freed on failure.
        unsafe {
            ffmpeg_sys_next::avcodec_free_context(&mut codec_ctx);
        }
        return Err(DecoderError::D3d11vaUnavailable);
    }

    // SAFETY: `codec_ctx` is live and owns the hardware-device reference after assignment.
    unsafe {
        (*codec_ctx).hw_device_ctx = hw_ref;
        (*codec_ctx).get_format = Some(d3d11_get_format);
    }

    // SAFETY: `codec_ctx` and `decoder` are valid FFmpeg decoder objects.
    let ret = unsafe { ffmpeg_sys_next::avcodec_open2(codec_ctx, decoder, std::ptr::null_mut()) };
    if ret < 0 {
        // SAFETY: `codec_ctx` owns `hw_ref` and must be freed on failure.
        unsafe {
            ffmpeg_sys_next::avcodec_free_context(&mut codec_ctx);
        }
        return Err(ffmpeg_err(ret));
    }

    Ok(AvCodecContext::from_ptr(codec_ctx))
}

/// Extracts the DXGI adapter LUID from FFmpeg's D3D11VA hardware device context.
pub(crate) fn adapter_luid_from_hw_device(
    hw_device: &AvHwDeviceRef,
    required_adapter: DxgiAdapterLuid,
) -> Result<DxgiAdapterLuid, DecoderError> {
    let actual = query_ffmpeg_d3d11_adapter_luid(hw_device)?;
    validate_same_adapter(required_adapter, actual).map_err(|err| match err {
        dvs_gpu::GpuError::AdapterLuidMismatch { expected, actual } => {
            DecoderError::AdapterLuidMismatch { expected, actual }
        }
        other => DecoderError::Gpu(other),
    })?;
    Ok(actual)
}

fn query_ffmpeg_d3d11_adapter_luid(
    hw_device: &AvHwDeviceRef,
) -> Result<DxgiAdapterLuid, DecoderError> {
    // SAFETY: `hw_device` is a live FFmpeg hardware device buffer reference.
    let device_ctx = unsafe {
        let buffer = hw_device
            .as_ptr()
            .as_ref()
            .ok_or(DecoderError::MissingD3d11Device)?;
        buffer.data as *const ffmpeg_sys_next::AVHWDeviceContext
    };
    if device_ctx.is_null() {
        return Err(DecoderError::MissingD3d11Device);
    }

    // SAFETY: `device_ctx` points at FFmpeg's `AVHWDeviceContext` payload.
    let hw_type = unsafe { (*device_ctx).type_ };
    if hw_type != ffmpeg_sys_next::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA {
        return Err(DecoderError::D3d11vaUnavailable);
    }

    // SAFETY: `hwctx` points at FFmpeg's D3D11VA device context; only `device` is read.
    let d3d11va = unsafe { (*device_ctx).hwctx as *const AvD3d11VaDeviceContext };
    if d3d11va.is_null() {
        return Err(DecoderError::MissingD3d11Device);
    }

    // SAFETY: `device` is owned by FFmpeg for the lifetime of the hardware device context.
    let raw_device = unsafe { (*d3d11va).device };
    if raw_device.is_null() {
        return Err(DecoderError::MissingD3d11Device);
    }

    // SAFETY: `raw_device` is a live COM pointer borrowed from FFmpeg's D3D11VA context.
    let d3d11_device = unsafe {
        windows::Win32::Graphics::Direct3D11::ID3D11Device::from_raw_borrowed(&raw_device)
            .ok_or(DecoderError::MissingD3d11Device)?
    };

    let dxgi_device: IDXGIDevice = d3d11_device
        .cast()
        .map_err(|_| DecoderError::AdapterQueryFailed)?;

    // SAFETY: `d3d11_device` is a live COM object; DXGI adapter queries are read-only metadata.
    unsafe {
        let adapter: IDXGIAdapter = dxgi_device
            .GetAdapter()
            .map_err(|_| DecoderError::AdapterQueryFailed)?;
        let desc = adapter
            .GetDesc()
            .map_err(|_| DecoderError::AdapterQueryFailed)?;
        Ok(DxgiAdapterLuid::new(
            desc.AdapterLuid.LowPart,
            desc.AdapterLuid.HighPart,
        ))
    }
}

/// Borrowed D3D11 decoder texture and allocation metadata from one decoded frame.
pub(crate) struct BorrowedD3d11DecoderSurface<'frame> {
    pub texture: &'frame ID3D11Texture2D,
    pub array_slice: u32,
    pub allocation_width: u32,
    pub allocation_height: u32,
}

/// Validates `AVFrame.data[0]`, borrows `ID3D11Texture2D`, and reads allocation metadata.
///
/// The returned borrow lifetime is tied to the decoder-owned `AVFrame` storage.
pub(crate) fn borrow_d3d11_decoder_surface<'frame>(
    frame: &'frame AvFrame,
) -> Result<BorrowedD3d11DecoderSurface<'frame>, DecoderError> {
    let frame_ptr = frame.as_ptr();
    // SAFETY: `frame.ptr` is a live decoded FFmpeg frame owned by the session.
    let frame_ref = unsafe { frame_ptr.as_ref() }.ok_or(DecoderError::InvalidDecoderState {
        detail: "decoded frame pointer is null",
    })?;

    if frame_ref.format != AV_PIX_FMT_D3D11 {
        return Err(DecoderError::UnexpectedPixelFormat {
            format: frame_ref.format,
        });
    }

    if frame_ref.data[0].is_null() {
        return Err(DecoderError::NullTexturePointer);
    }

    let array_slice_usize = frame_ref.data[1] as usize;
    let array_slice =
        u32::try_from(array_slice_usize).map_err(|_| DecoderError::InvalidTextureArraySlice {
            index: array_slice_usize as i64,
        })?;

    // SAFETY: `data[0]` originates from a received `AV_PIX_FMT_D3D11` frame; FFmpeg owns the
    // texture and the current `AVFrame` retains it; `from_raw_borrowed` does not take ownership
    // or call `Release`; the pointer slot and COM object remain valid for the returned Rust
    // borrow; the mutable decoder borrow prevents frame reuse while the surface exists; null and
    // alignment checks have already been performed.
    let texture = unsafe {
        let slot = std::ptr::addr_of!((*frame_ptr).data[0]);
        let com_pointer_slot = &*slot.cast::<*mut c_void>();
        ID3D11Texture2D::from_raw_borrowed(com_pointer_slot)
            .ok_or(DecoderError::NullTexturePointer)?
    };

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: `texture` is borrowed from the frame's `data[0]` slot for descriptor read only.
    unsafe {
        texture.GetDesc(&mut desc);
    }

    if desc.Width == 0 || desc.Height == 0 {
        return Err(DecoderError::UnsupportedTextureLayout);
    }

    Ok(BorrowedD3d11DecoderSurface {
        texture,
        array_slice,
        allocation_width: desc.Width,
        allocation_height: desc.Height,
    })
}

/// Reads visible crop and color metadata fields from a decoded frame.
pub(crate) struct FrameFields {
    pub pts: i64,
    pub visible_x: u32,
    pub visible_y: u32,
    pub visible_width: u32,
    pub visible_height: u32,
    pub color_range: i32,
    pub colorspace: i32,
    pub color_primaries: i32,
    pub color_trc: i32,
}

pub(crate) fn read_frame_fields(frame: &AvFrame) -> Result<FrameFields, DecoderError> {
    // SAFETY: `frame.ptr` is a live decoded FFmpeg frame owned by the session.
    let frame_ptr =
        unsafe { frame.as_ptr().as_ref() }.ok_or(DecoderError::InvalidDecoderState {
            detail: "decoded frame pointer is null",
        })?;

    let coded_width =
        u32::try_from(frame_ptr.width).map_err(|_| DecoderError::InvalidDecoderState {
            detail: "frame width is negative",
        })?;
    let coded_height =
        u32::try_from(frame_ptr.height).map_err(|_| DecoderError::InvalidDecoderState {
            detail: "frame height is negative",
        })?;

    let crop_left =
        u32::try_from(frame_ptr.crop_left).map_err(|_| DecoderError::InvalidDecoderState {
            detail: "crop_left is negative",
        })?;
    let crop_top =
        u32::try_from(frame_ptr.crop_top).map_err(|_| DecoderError::InvalidDecoderState {
            detail: "crop_top is negative",
        })?;
    let crop_right =
        u32::try_from(frame_ptr.crop_right).map_err(|_| DecoderError::InvalidDecoderState {
            detail: "crop_right is negative",
        })?;
    let crop_bottom =
        u32::try_from(frame_ptr.crop_bottom).map_err(|_| DecoderError::InvalidDecoderState {
            detail: "crop_bottom is negative",
        })?;

    let visible_width = coded_width
        .checked_sub(crop_left)
        .and_then(|w| w.checked_sub(crop_right))
        .ok_or(DecoderError::InvalidDecoderState {
            detail: "visible width underflow",
        })?;
    let visible_height = coded_height
        .checked_sub(crop_top)
        .and_then(|h| h.checked_sub(crop_bottom))
        .ok_or(DecoderError::InvalidDecoderState {
            detail: "visible height underflow",
        })?;

    if visible_width == 0 || visible_height == 0 {
        return Err(DecoderError::InvalidDecoderState {
            detail: "visible dimensions are zero",
        });
    }

    Ok(FrameFields {
        pts: frame_ptr.pts,
        visible_x: crop_left,
        visible_y: crop_top,
        visible_width,
        visible_height,
        color_range: frame_ptr.color_range as i32,
        colorspace: frame_ptr.colorspace as i32,
        color_primaries: frame_ptr.color_primaries as i32,
        color_trc: frame_ptr.color_trc as i32,
    })
}
