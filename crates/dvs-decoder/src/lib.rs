//! Production FFmpeg D3D11VA decoder session (Windows).

#![deny(unsafe_code)]

mod error;
mod metadata;

#[cfg(windows)]
mod ffmpeg;
#[cfg(windows)]
mod session;

pub use error::DecoderError;
pub use metadata::{
    AV_NOPTS_VALUE, build_dimensions, build_frame_metadata, color_info_from_ffmpeg,
    map_color_matrix, map_color_primaries, map_color_range, map_transfer_characteristic,
    next_frame_id, pts_to_timestamp,
};

#[cfg(windows)]
pub use session::{DecodedD3d11Frame, DecoderD3d11Hardware, DecoderSession};
