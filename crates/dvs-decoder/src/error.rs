//! Typed decoder errors.

use dvs_gpu::DxgiAdapterLuid;
use thiserror::Error;

/// Errors produced by the production decoder session.
#[derive(Debug, Error)]
pub enum DecoderError {
    /// Input path does not exist or is not accessible.
    #[error("input path not found: {path}")]
    InputPathNotFound { path: String },

    /// FFmpeg returned a negative status code.
    #[error("FFmpeg error {code}: {message}")]
    Ffmpeg { code: i32, message: String },

    /// D3D11VA hardware device creation failed or is unavailable.
    #[error("D3D11VA hardware acceleration is unavailable")]
    D3d11vaUnavailable,

    /// `get_format` did not select `AV_PIX_FMT_D3D11`.
    #[error("AV_PIX_FMT_D3D11 was not offered or not selected by the decoder")]
    D3d11PixelFormatUnavailable,

    /// Decoded frame format was not `AV_PIX_FMT_D3D11`.
    #[error("decoded frame format is not AV_PIX_FMT_D3D11 (got format {format})")]
    UnexpectedPixelFormat { format: i32 },

    /// FFmpeg D3D11VA device context is missing or null.
    #[error("FFmpeg D3D11VA device is missing or null")]
    MissingD3d11Device,

    /// Required wgpu adapter LUID does not match the FFmpeg D3D11 adapter.
    #[error("adapter LUID mismatch: expected {expected}, actual {actual}")]
    AdapterLuidMismatch {
        expected: DxgiAdapterLuid,
        actual: DxgiAdapterLuid,
    },

    /// `AVFrame.data[0]` did not contain a D3D11 texture pointer.
    #[error("D3D11 texture pointer in AVFrame.data[0] is null")]
    NullTexturePointer,

    /// `AVFrame.data[1]` array slice index is invalid.
    #[error("invalid D3D11 texture array slice index: {index}")]
    InvalidTextureArraySlice { index: i64 },

    /// Texture descriptor or layout is unsupported for NV12 decode output.
    #[error("unsupported D3D11 decoder texture format or layout")]
    UnsupportedTextureLayout,

    /// Metadata construction failed (dimensions, time base, crop).
    #[error("invalid frame metadata: {0}")]
    InvalidMetadata(#[from] dvs_media::MetadataError),

    /// Stream or codec discovery failed.
    #[error("no suitable video stream found")]
    NoVideoStream,

    /// Decoder state is inconsistent (for example, unexpected `EAGAIN` during drain).
    #[error("decoder state is invalid: {detail}")]
    InvalidDecoderState { detail: &'static str },

    /// DXGI adapter query from the FFmpeg D3D11 device failed.
    #[error("failed to query DXGI adapter LUID from FFmpeg D3D11 device")]
    AdapterQueryFailed,

    /// Borrowed surface validation failed in `dvs-gpu`.
    #[error("GPU surface validation failed: {0}")]
    Gpu(#[from] dvs_gpu::GpuError),
}

impl DecoderError {
    pub(crate) fn ffmpeg(code: i32, message: impl Into<String>) -> Self {
        Self::Ffmpeg {
            code,
            message: message.into(),
        }
    }
}

impl From<dvs_gpu::D3d11ExternalContextLockConfigError> for DecoderError {
    fn from(error: dvs_gpu::D3d11ExternalContextLockConfigError) -> Self {
        Self::Gpu(dvs_gpu::GpuError::D3d11ExternalContextLockConfigInvalid { error })
    }
}
