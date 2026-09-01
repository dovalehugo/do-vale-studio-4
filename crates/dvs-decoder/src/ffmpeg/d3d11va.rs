//! FFmpeg D3D11VA device creation, adapter validation, and surface borrowing.

use std::ffi::c_void;

use dvs_gpu::{DxgiAdapterLuid, validate_same_adapter};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::core::Interface;

use crate::error::DecoderError;
use crate::ffmpeg::ffi::ffmpeg_err;
use crate::ffmpeg::raii::{AvCodecContext, AvFrame, AvHwDeviceRef};

const AV_PIX_FMT_D3D11: i32 = ffmpeg_sys_next::AVPixelFormat::AV_PIX_FMT_D3D11 as i32;
const AV_PIX_FMT_NONE: ffmpeg_sys_next::AVPixelFormat =
    ffmpeg_sys_next::AVPixelFormat::AV_PIX_FMT_NONE;

/// FFmpeg `AVD3D11VADeviceContext` layout (prefix fields used by the decoder).
#[repr(C)]
struct AvD3d11VaDeviceContext {
    device: *mut c_void,
    device_context: *mut c_void,
    video_device: *mut c_void,
    video_context: *mut c_void,
    lock: Option<unsafe extern "C" fn(*mut c_void)>,
    unlock: Option<unsafe extern "C" fn(*mut c_void)>,
    lock_ctx: *mut c_void,
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

fn ffmpeg_d3d11va_context_ptr(
    hw_device: &AvHwDeviceRef,
) -> Result<*const AvD3d11VaDeviceContext, DecoderError> {
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

    // SAFETY: `hwctx` points at FFmpeg's D3D11VA device context for the session lifetime.
    let d3d11va = unsafe { (*device_ctx).hwctx as *const AvD3d11VaDeviceContext };
    if d3d11va.is_null() {
        return Err(DecoderError::MissingD3d11Device);
    }

    Ok(d3d11va)
}

/// Borrows FFmpeg's D3D11VA `ID3D11Device` for interop producer setup.
pub(crate) fn borrow_ffmpeg_d3d11_device(
    hw_device: &AvHwDeviceRef,
) -> Result<&ID3D11Device, DecoderError> {
    let d3d11va = ffmpeg_d3d11va_context_ptr(hw_device)?;

    // SAFETY: `device` is owned by FFmpeg for the lifetime of the hardware device context;
    // `from_raw_borrowed` does not take ownership; the pointer slot remains valid for `'a`.
    unsafe {
        let slot = std::ptr::addr_of!((*d3d11va).device);
        let com_pointer_slot = &*slot.cast::<*mut c_void>();
        ID3D11Device::from_raw_borrowed(com_pointer_slot).ok_or(DecoderError::MissingD3d11Device)
    }
}

/// FFmpeg lock callbacks installed on `AVD3D11VADeviceContext` during hwdevice init.
pub(crate) struct FfmpegD3d11DeviceLock {
    lock: Option<unsafe extern "C" fn(*mut c_void)>,
    unlock: Option<unsafe extern "C" fn(*mut c_void)>,
    lock_ctx: *mut c_void,
}

impl FfmpegD3d11DeviceLock {
    pub(crate) fn from_hw_device(hw_device: &AvHwDeviceRef) -> Result<Self, DecoderError> {
        let d3d11va = ffmpeg_d3d11va_context_ptr(hw_device)?;
        // SAFETY: `d3d11va` points at FFmpeg's initialized D3D11VA device context.
        Ok(unsafe {
            Self {
                lock: (*d3d11va).lock,
                unlock: (*d3d11va).unlock,
                lock_ctx: (*d3d11va).lock_ctx,
            }
        })
    }

    /// Converts to the interop producer lock type with an owned hardware-device keepalive.
    pub(crate) fn to_external(
        &self,
        keepalive: dvs_gpu::D3d11ExternalContextLockKeepalive,
    ) -> Result<dvs_gpu::D3d11ExternalContextLock, dvs_gpu::D3d11ExternalContextLockConfigError>
    {
        // SAFETY: `lock`/`unlock`/`lock_ctx` are read from the live `AVD3D11VADeviceContext`
        // owned by `keepalive`'s retained `AVBufferRef`; callbacks are FFmpeg's matching pair;
        // `lock_ctx` remains valid until the keepalive `AVBufferRef` is released; callbacks
        // obey FFmpeg's recursive lock protocol and do not unwind across FFI; acquire/release
        // are only used from the decoder-thread producer path with RAII pairing.
        unsafe {
            dvs_gpu::D3d11ExternalContextLock::new_with_keepalive(
                self.lock,
                self.unlock,
                self.lock_ctx,
                keepalive,
            )
        }
    }
}

/// Builds a producer-side FFmpeg `device_context` lock with an owned hardware-device keepalive.
pub(crate) fn build_external_context_lock(
    hw_device: &AvHwDeviceRef,
) -> Result<dvs_gpu::D3d11ExternalContextLock, DecoderError> {
    let ffmpeg_lock = FfmpegD3d11DeviceLock::from_hw_device(hw_device)?;
    let keepalive = dvs_gpu::D3d11ExternalContextLockKeepalive::new(hw_device.retain_ref()?);
    ffmpeg_lock
        .to_external(keepalive)
        .map_err(DecoderError::from)
}

/// Clones FFmpeg's `AVD3D11VADeviceContext.device_context` COM reference.
///
/// FFmpeg populates this field via `ID3D11Device::GetImmediateContext` during hwdevice init.
pub(crate) fn clone_ffmpeg_d3d11_device_context(
    hw_device: &AvHwDeviceRef,
) -> Result<ID3D11DeviceContext, DecoderError> {
    let d3d11va = ffmpeg_d3d11va_context_ptr(hw_device)?;
    // SAFETY: `device_context` is populated by FFmpeg during `av_hwdevice_ctx_create` init.
    let raw_context = unsafe { (*d3d11va).device_context };
    if raw_context.is_null() {
        return Err(DecoderError::MissingD3d11Device);
    }
    // SAFETY: `device_context` slot references FFmpeg's immediate context; `from_raw_borrowed`
    // does not take ownership; `clone` AddRefs for session storage.
    unsafe {
        let slot = std::ptr::addr_of!((*d3d11va).device_context);
        let borrowed = ID3D11DeviceContext::from_raw_borrowed(&*slot.cast::<*mut c_void>())
            .ok_or(DecoderError::MissingD3d11Device)?;
        Ok(borrowed.clone())
    }
}

#[cfg(debug_assertions)]
pub(crate) fn ffmpeg_d3d11_device_context_ptr(
    hw_device: &AvHwDeviceRef,
) -> Result<*mut c_void, DecoderError> {
    let d3d11va = ffmpeg_d3d11va_context_ptr(hw_device)?;
    // SAFETY: `device_context` is FFmpeg's immediate context pointer slot.
    let raw = unsafe { (*d3d11va).device_context };
    if raw.is_null() {
        return Err(DecoderError::MissingD3d11Device);
    }
    Ok(raw)
}

fn query_ffmpeg_d3d11_adapter_luid(
    hw_device: &AvHwDeviceRef,
) -> Result<DxgiAdapterLuid, DecoderError> {
    let d3d11_device = borrow_ffmpeg_d3d11_device(hw_device)?;

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
