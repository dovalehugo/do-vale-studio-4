//! Typed application errors for the production composition root.

use std::path::PathBuf;

use thiserror::Error;

/// Error returned by the production application entry points.
#[derive(Debug, Error)]
pub enum AppError {
    /// CLI parsing or configuration validation failed.
    #[error("configuration error: {0}")]
    Config(String),

    /// The requested input path is missing or not a regular file.
    #[error("invalid input path: {path}")]
    InvalidInput { path: PathBuf },

    /// The current platform is not supported for the production video path.
    #[error("unsupported platform for production video playback")]
    UnsupportedPlatform,

    /// Window or event-loop creation failed.
    #[error("window creation failed: {0}")]
    Window(String),

    /// GPU bootstrap or surface initialization failed.
    #[error("GPU initialization failed: {0}")]
    Gpu(#[from] dvs_gpu::GpuError),

    /// Decoder session or decode operation failed.
    #[error("decoder error: {0}")]
    Decoder(#[from] dvs_decoder::DecoderError),

    /// Renderer or surface presentation failed.
    #[error("render error: {0}")]
    Render(#[from] dvs_render::RenderError),

    /// Playback clock or scheduler rejected a timestamp.
    #[error("playback error: {0}")]
    Playback(#[from] dvs_playback::PlaybackError),

    /// Application state transition was invalid.
    #[error("invalid application state transition")]
    InvalidState,

    /// Fatal runtime condition such as device loss or out-of-memory.
    #[error("fatal runtime error: {0}")]
    Fatal(String),
}
