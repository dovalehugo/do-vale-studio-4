//! RAII wrappers for FFmpeg objects.

use crate::error::DecoderError;
use crate::ffmpeg::ffi::ffmpeg_err;

/// Initializes FFmpeg network support once per session.
pub(crate) fn init_ffmpeg_network() -> Result<(), DecoderError> {
    // SAFETY: FFmpeg global network setup is reference-counted and safe to call per session.
    let ret = unsafe { ffmpeg_sys_next::avformat_network_init() };
    if ret < 0 {
        return Err(ffmpeg_err(ret));
    }
    Ok(())
}

/// Owns an `AVFormatContext` and closes it exactly once on drop.
pub(crate) struct AvFormatContext {
    ptr: *mut ffmpeg_sys_next::AVFormatContext,
}

impl AvFormatContext {
    pub(crate) fn open(path: &std::path::Path) -> Result<Self, DecoderError> {
        if !path.exists() {
            return Err(DecoderError::InputPathNotFound {
                path: path.display().to_string(),
            });
        }

        let path_c = std::ffi::CString::new(path.to_string_lossy().as_ref()).map_err(|_| {
            DecoderError::InvalidDecoderState {
                detail: "input path contains interior NUL byte",
            }
        })?;

        let mut ptr: *mut ffmpeg_sys_next::AVFormatContext = std::ptr::null_mut();
        // SAFETY: `path_c` is NUL-terminated; output pointer starts null.
        let ret = unsafe {
            ffmpeg_sys_next::avformat_open_input(
                &mut ptr,
                path_c.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        if ret < 0 {
            return Err(ffmpeg_err(ret));
        }

        Ok(Self { ptr })
    }

    pub(crate) fn discover_streams(&self) -> Result<(), DecoderError> {
        // SAFETY: `ptr` is a live format context opened by FFmpeg.
        let ret =
            unsafe { ffmpeg_sys_next::avformat_find_stream_info(self.ptr, std::ptr::null_mut()) };
        if ret < 0 {
            return Err(ffmpeg_err(ret));
        }
        Ok(())
    }

    pub(crate) fn find_best_video_stream(&self) -> Result<i32, DecoderError> {
        // SAFETY: `ptr` is a live format context with stream info discovered.
        let stream_index = unsafe {
            ffmpeg_sys_next::av_find_best_stream(
                self.ptr,
                ffmpeg_sys_next::AVMediaType::AVMEDIA_TYPE_VIDEO,
                -1,
                -1,
                std::ptr::null_mut(),
                0,
            )
        };
        if stream_index < 0 {
            return Err(ffmpeg_err(stream_index));
        }
        Ok(stream_index)
    }

    pub(crate) fn stream_time_base(&self, stream_index: i32) -> Result<(i32, i32), DecoderError> {
        // SAFETY: `ptr` owns streams; `stream_index` was validated by discovery.
        unsafe {
            let fmt = self.ptr.as_ref().ok_or(DecoderError::InvalidDecoderState {
                detail: "format context is null",
            })?;
            if stream_index < 0 || stream_index as u32 >= fmt.nb_streams {
                return Err(DecoderError::InvalidDecoderState {
                    detail: "video stream index out of bounds",
                });
            }
            let streams = fmt.streams;
            let stream = (*streams.add(stream_index as usize)).as_ref().ok_or(
                DecoderError::InvalidDecoderState {
                    detail: "video stream pointer is null",
                },
            )?;
            let tb = stream.time_base;
            if tb.num <= 0 || tb.den <= 0 {
                return Err(DecoderError::InvalidDecoderState {
                    detail: "stream time base is invalid",
                });
            }
            Ok((tb.num, tb.den))
        }
    }

    pub(crate) fn codec_parameters(
        &self,
        stream_index: i32,
    ) -> Result<*const ffmpeg_sys_next::AVCodecParameters, DecoderError> {
        // SAFETY: `ptr` owns streams; `stream_index` was validated by discovery.
        unsafe {
            let fmt = self.ptr.as_ref().ok_or(DecoderError::InvalidDecoderState {
                detail: "format context is null",
            })?;
            if stream_index < 0 || stream_index as u32 >= fmt.nb_streams {
                return Err(DecoderError::InvalidDecoderState {
                    detail: "video stream index out of bounds",
                });
            }
            let stream = (*fmt.streams.add(stream_index as usize)).as_ref().ok_or(
                DecoderError::InvalidDecoderState {
                    detail: "video stream pointer is null",
                },
            )?;
            let codecpar = stream.codecpar;
            if codecpar.is_null() {
                return Err(DecoderError::InvalidDecoderState {
                    detail: "codec parameters are null",
                });
            }
            Ok(codecpar)
        }
    }

    pub(crate) fn read_packet(
        &self,
        packet: &mut AvPacket,
    ) -> Result<ReadPacketResult, DecoderError> {
        // SAFETY: `ptr` and `packet.ptr` are live FFmpeg objects owned by the session.
        let ret = unsafe { ffmpeg_sys_next::av_read_frame(self.ptr, packet.as_mut_ptr()) };
        if crate::ffmpeg::ffi::is_eof(ret) {
            Ok(ReadPacketResult::Eof)
        } else if ret < 0 {
            Err(ffmpeg_err(ret))
        } else {
            Ok(ReadPacketResult::Packet)
        }
    }
}

impl Drop for AvFormatContext {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: `ptr` is owned and only freed here once.
            unsafe {
                ffmpeg_sys_next::avformat_close_input(&mut self.ptr);
            }
        }
    }
}

/// Owns an `AVBufferRef` to an `AVHWDeviceContext`.
pub(crate) struct AvHwDeviceRef {
    ptr: *mut ffmpeg_sys_next::AVBufferRef,
}

impl AvHwDeviceRef {
    pub(crate) fn from_ptr(ptr: *mut ffmpeg_sys_next::AVBufferRef) -> Self {
        Self { ptr }
    }

    pub(crate) fn as_ptr(&self) -> *mut ffmpeg_sys_next::AVBufferRef {
        self.ptr
    }
}

impl Drop for AvHwDeviceRef {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: `ptr` is an owned FFmpeg buffer reference.
            unsafe {
                ffmpeg_sys_next::av_buffer_unref(&mut self.ptr);
            }
        }
    }
}

/// Owns an `AVCodecContext` and frees it exactly once on drop.
pub(crate) struct AvCodecContext {
    ptr: *mut ffmpeg_sys_next::AVCodecContext,
}

impl AvCodecContext {
    pub(crate) fn from_ptr(ptr: *mut ffmpeg_sys_next::AVCodecContext) -> Self {
        Self { ptr }
    }

    pub(crate) fn receive_frame(&self, frame: &mut AvFrame) -> Result<ReceiveResult, DecoderError> {
        // SAFETY: `ptr` and `frame.ptr` are live FFmpeg objects owned by the session.
        let ret = unsafe { ffmpeg_sys_next::avcodec_receive_frame(self.ptr, frame.as_mut_ptr()) };
        if ret == 0 {
            Ok(ReceiveResult::Frame)
        } else if crate::ffmpeg::ffi::is_eagain(ret) {
            Ok(ReceiveResult::Again)
        } else if crate::ffmpeg::ffi::is_eof(ret) {
            Ok(ReceiveResult::Eof)
        } else {
            Err(ffmpeg_err(ret))
        }
    }

    pub(crate) fn send_packet(
        &self,
        packet: Option<&AvPacket>,
    ) -> Result<SendResult, DecoderError> {
        let packet_ptr = packet.map(|p| p.as_ptr()).unwrap_or(std::ptr::null_mut());
        // SAFETY: `ptr` is live; packet pointer is either null (flush) or a valid packet.
        let ret = unsafe { ffmpeg_sys_next::avcodec_send_packet(self.ptr, packet_ptr) };
        if ret == 0 {
            Ok(SendResult::Accepted)
        } else if crate::ffmpeg::ffi::is_eagain(ret) {
            Ok(SendResult::Again)
        } else {
            Err(ffmpeg_err(ret))
        }
    }
}

/// Result of `avcodec_receive_frame`.
pub(crate) enum ReceiveResult {
    Frame,
    Again,
    Eof,
}

/// Result of `avcodec_send_packet`.
pub(crate) enum SendResult {
    Accepted,
    Again,
}

/// Result of `av_read_frame`.
pub(crate) enum ReadPacketResult {
    Packet,
    Eof,
}

impl Drop for AvCodecContext {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: `ptr` is owned and only freed here once.
            unsafe {
                ffmpeg_sys_next::avcodec_free_context(&mut self.ptr);
            }
        }
    }
}

/// Owns an `AVPacket` and unrefs it on drop.
pub(crate) struct AvPacket {
    ptr: *mut ffmpeg_sys_next::AVPacket,
}

impl AvPacket {
    pub(crate) fn new() -> Result<Self, DecoderError> {
        // SAFETY: FFmpeg allocates a packet object.
        let ptr = unsafe { ffmpeg_sys_next::av_packet_alloc() };
        if ptr.is_null() {
            return Err(DecoderError::InvalidDecoderState {
                detail: "av_packet_alloc failed",
            });
        }
        Ok(Self { ptr })
    }

    pub(crate) fn as_ptr(&self) -> *mut ffmpeg_sys_next::AVPacket {
        self.ptr
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut ffmpeg_sys_next::AVPacket {
        self.ptr
    }

    pub(crate) fn unref(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: `ptr` is an owned FFmpeg packet.
            unsafe {
                ffmpeg_sys_next::av_packet_unref(self.ptr);
            }
        }
    }

    pub(crate) fn stream_index(&self) -> i32 {
        // SAFETY: `ptr` is a live packet.
        unsafe { (*self.ptr).stream_index }
    }
}

impl Drop for AvPacket {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.unref();
            // SAFETY: `ptr` is an owned FFmpeg packet object.
            unsafe {
                ffmpeg_sys_next::av_packet_free(&mut self.ptr);
            }
        }
    }
}

/// Owns an `AVFrame` and unrefs/frees it on drop.
pub(crate) struct AvFrame {
    ptr: *mut ffmpeg_sys_next::AVFrame,
}

impl AvFrame {
    pub(crate) fn new() -> Result<Self, DecoderError> {
        // SAFETY: FFmpeg allocates a frame object.
        let ptr = unsafe { ffmpeg_sys_next::av_frame_alloc() };
        if ptr.is_null() {
            return Err(DecoderError::InvalidDecoderState {
                detail: "av_frame_alloc failed",
            });
        }
        Ok(Self { ptr })
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut ffmpeg_sys_next::AVFrame {
        self.ptr
    }

    pub(crate) fn as_ptr(&self) -> *const ffmpeg_sys_next::AVFrame {
        self.ptr
    }

    pub(crate) fn unref(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: `ptr` is an owned FFmpeg frame.
            unsafe {
                ffmpeg_sys_next::av_frame_unref(self.ptr);
            }
        }
    }
}

impl Drop for AvFrame {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.unref();
            // SAFETY: `ptr` is an owned FFmpeg frame object.
            unsafe {
                ffmpeg_sys_next::av_frame_free(&mut self.ptr);
            }
        }
    }
}
