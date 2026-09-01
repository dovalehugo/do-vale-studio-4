//! FFmpeg error code helpers.

use crate::error::DecoderError;

pub(crate) fn is_eagain(code: i32) -> bool {
    code == ffmpeg_sys_next::AVERROR(ffmpeg_sys_next::EAGAIN)
}

pub(crate) fn is_eof(code: i32) -> bool {
    code == ffmpeg_sys_next::AVERROR_EOF
}

pub(crate) fn ffmpeg_err(code: i32) -> DecoderError {
    let mut buf = [0i8; ffmpeg_sys_next::AV_ERROR_MAX_STRING_SIZE];
    // SAFETY: `buf` is a valid stack buffer of the documented FFmpeg error-string size.
    let written = unsafe { ffmpeg_sys_next::av_strerror(code, buf.as_mut_ptr(), buf.len()) };
    let message = if written < 0 {
        format!("FFmpeg error code {code}")
    } else {
        // SAFETY: `av_strerror` wrote a NUL-terminated C string into `buf` on success.
        unsafe {
            std::ffi::CStr::from_ptr(buf.as_ptr())
                .to_string_lossy()
                .into_owned()
        }
    };
    DecoderError::ffmpeg(code, message)
}
