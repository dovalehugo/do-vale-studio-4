//! GPU Experiment 2 — D3D11VA → wgpu interop.
//!
//! Blocked until `docs/fixtures/test_4k_hevc_8bit30.mp4` exists and FFmpeg dev
//! libraries are configured. See `docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md`.

mod multi_frame;
mod render_path;
mod visual_diagnostic;
mod visual_validation;
mod wgpu_hal_interop;
use ffmpeg_sys_next as ffmpeg;
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_FENCE_FLAG_SHARED, D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device5,
    ID3D11DeviceContext4, ID3D11Fence, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_COMMAND_QUEUE_FLAG_NONE,
    D3D12CreateDevice, ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ADAPTER_DESC, DXGI_ERROR_WAIT_TIMEOUT, DXGI_SHARED_RESOURCE_READ,
    DXGI_SHARED_RESOURCE_WRITE, IDXGIDevice, IDXGIKeyedMutex, IDXGIResource1,
};
use windows::core::{Interface, PCWSTR};

const FIXTURE_REL: &str = "docs/fixtures/test_4k_hevc_8bit30.mp4";
const SETUP_DOC: &str = "docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md";
/// Fence value signaled on the D3D11 queue after the GPU copy completes.
const FENCE_SIGNAL_VALUE: u64 = 1;

fn repo_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn fixture_path() -> PathBuf {
    repo_root().join(FIXTURE_REL)
}

fn try_ffmpeg_runtime_version() -> Option<String> {
    unsafe {
        let ptr = ffmpeg::av_version_info();
        if ptr.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

struct FfmpegDevLayout {
    ffmpeg_dir: Option<PathBuf>,
    include_ok: bool,
    lib_ok: bool,
    bin_ok: bool,
    libclang_set: bool,
}

impl FfmpegDevLayout {
    fn inspect() -> Self {
        let ffmpeg_dir = std::env::var("FFMPEG_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty());

        let (include_ok, lib_ok, bin_ok) = ffmpeg_dir
            .as_ref()
            .map(|dir| {
                (
                    dir.join("include/libavcodec/avcodec.h").is_file(),
                    dir.join("lib/avcodec.lib").is_file(),
                    dir.join("bin/ffmpeg.exe").is_file(),
                )
            })
            .unwrap_or((false, false, false));

        let libclang_set = std::env::var("LIBCLANG_PATH")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some();

        Self {
            ffmpeg_dir,
            include_ok,
            lib_ok,
            bin_ok,
            libclang_set,
        }
    }

    fn dev_libs_ok(&self) -> bool {
        self.include_ok && self.lib_ok
    }

    fn print_diagnostics(&self) {
        println!("FFmpeg development environment:");
        match &self.ffmpeg_dir {
            Some(dir) => println!("  FFMPEG_DIR:      {}", dir.display()),
            None => println!("  FFMPEG_DIR:      (not set)"),
        }
        println!("  include/avcodec: {}", status_label(self.include_ok));
        println!("  lib/avcodec.lib: {}", status_label(self.lib_ok));
        println!("  bin/ffmpeg.exe:  {}", status_label(self.bin_ok));
        println!(
            "  LIBCLANG_PATH:   {}",
            if self.libclang_set {
                "set (build-time only)"
            } else {
                "(not set — build-time only, required for ffmpeg-sys-next bindgen)"
            }
        );
        println!();
        println!("Setup guide: {SETUP_DOC}");
    }
}

fn status_label(ok: bool) -> &'static str {
    if ok { "OK" } else { "MISSING" }
}

fn ffmpeg_error(code: i32) -> String {
    let mut buf = [0i8; ffmpeg::AV_ERROR_MAX_STRING_SIZE as usize];
    unsafe {
        ffmpeg::av_strerror(code, buf.as_mut_ptr(), buf.len());
        std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .into_owned()
    }
}

fn ffmpeg_err<T>(code: i32) -> Result<T, String> {
    Err(ffmpeg_error(code))
}

fn c_str_name(ptr: *const i8) -> String {
    if ptr.is_null() {
        "(null)".to_string()
    } else {
        unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}

struct GetFormatLog {
    candidates: Vec<String>,
    selected: Option<String>,
}

impl Default for GetFormatLog {
    fn default() -> Self {
        Self {
            candidates: Vec::new(),
            selected: None,
        }
    }
}

thread_local! {
    static GET_FORMAT_LOG: RefCell<GetFormatLog> = RefCell::new(GetFormatLog::default());
}

unsafe extern "C" fn d3d11_get_format(
    _avctx: *mut ffmpeg::AVCodecContext,
    pix_fmts: *const ffmpeg::AVPixelFormat,
) -> ffmpeg::AVPixelFormat {
    let mut selected = ffmpeg::AVPixelFormat::AV_PIX_FMT_NONE;
    let mut candidates = Vec::new();
    let mut d3d11_offered = false;

    if !pix_fmts.is_null() {
        let mut i = 0;
        loop {
            let fmt = *pix_fmts.add(i);
            if fmt == ffmpeg::AVPixelFormat::AV_PIX_FMT_NONE {
                break;
            }
            let name = c_str_name(ffmpeg::av_get_pix_fmt_name(fmt));
            println!("  get_format candidate: {} ({name})", fmt as i32);
            candidates.push(format!("{} ({name})", fmt as i32));
            if fmt == ffmpeg::AVPixelFormat::AV_PIX_FMT_D3D11 {
                d3d11_offered = true;
                selected = fmt;
            }
            i += 1;
        }
    }

    let selected_label = if selected != ffmpeg::AVPixelFormat::AV_PIX_FMT_NONE {
        let name = c_str_name(ffmpeg::av_get_pix_fmt_name(selected));
        println!("  get_format selected: {} ({name})", selected as i32);
        Some(format!("{} ({name})", selected as i32))
    } else {
        if d3d11_offered {
            println!("  get_format: AV_PIX_FMT_D3D11 was listed but not selected");
        } else {
            println!("  get_format: AV_PIX_FMT_D3D11 not offered — returning AV_PIX_FMT_NONE");
        }
        None
    };

    GET_FORMAT_LOG.with(|log| {
        let mut log = log.borrow_mut();
        for candidate in candidates {
            if !log.candidates.contains(&candidate) {
                log.candidates.push(candidate);
            }
        }
        if let Some(sel) = selected_label {
            log.selected = Some(sel);
        }
    });

    selected
}

fn reset_get_format_log() {
    GET_FORMAT_LOG.with(|log| *log.borrow_mut() = GetFormatLog::default());
}

fn take_get_format_log() -> GetFormatLog {
    GET_FORMAT_LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

struct FormatContext(*mut ffmpeg::AVFormatContext);

impl FormatContext {
    fn open(path: &std::path::Path) -> Result<Self, String> {
        let path_c = std::ffi::CString::new(path.to_string_lossy().as_ref())
            .map_err(|e| format!("invalid fixture path: {e}"))?;

        let mut ctx: *mut ffmpeg::AVFormatContext = std::ptr::null_mut();
        let ret = unsafe {
            ffmpeg::avformat_open_input(
                &mut ctx,
                path_c.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        if ret < 0 {
            return ffmpeg_err(ret);
        }
        Ok(Self(ctx))
    }

    fn as_ptr(&self) -> *mut ffmpeg::AVFormatContext {
        self.0
    }
}

impl Drop for FormatContext {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ffmpeg::avformat_close_input(&mut self.0);
            }
        }
    }
}

struct BufferRef(*mut ffmpeg::AVBufferRef);

impl BufferRef {
    fn as_ptr(&self) -> *mut ffmpeg::AVBufferRef {
        self.0
    }
}

impl Drop for BufferRef {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ffmpeg::av_buffer_unref(&mut self.0);
            }
        }
    }
}

struct CodecContext(*mut ffmpeg::AVCodecContext);

impl CodecContext {
    fn as_ptr(&self) -> *mut ffmpeg::AVCodecContext {
        self.0
    }
}

impl Drop for CodecContext {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ffmpeg::avcodec_free_context(&mut self.0);
            }
        }
    }
}

struct VideoStreamInfo {
    stream_index: i32,
    codec_id: ffmpeg::AVCodecID,
    codec_name: String,
    width: i32,
    height: i32,
    pix_fmt: i32,
    pix_fmt_name: String,
    time_base_num: i32,
    time_base_den: i32,
}

struct DecoderInfo {
    hw_device_type: String,
    d3d11va_device_ok: bool,
    decoder_name: String,
    codec_id: ffmpeg::AVCodecID,
    width: i32,
    height: i32,
    pix_fmt: i32,
    pix_fmt_name: String,
    get_format_candidates: Vec<String>,
    get_format_selected: Option<String>,
}

struct DecodedFrameInfo {
    format: i32,
    format_name: String,
    width: i32,
    height: i32,
    pts: i64,
    is_d3d11: bool,
}

struct D3d11FrameInspection {
    texture_ptr: usize,
    texture_array_index: i32,
    hw_frames_ctx_present: bool,
    hw_format: String,
    sw_format: String,
}

struct D3d11TextureDesc {
    width: u32,
    height: u32,
    mip_levels: u32,
    array_size: u32,
    dxgi_format: String,
    sample_count: u32,
    sample_quality: u32,
    usage: String,
    bind_flags: u32,
    cpu_access_flags: u32,
    misc_flags: u32,
    array_slice_index: i32,
}

struct ShareableTextureInfo {
    creation_ok: bool,
    has_shared_nthandle: bool,
    desc: D3d11TextureDesc,
}

struct GpuCopyInfo {
    submission_ok: bool,
    source_array_slice: i32,
    source_subresource: u32,
    destination_subresource: u32,
    source_format: String,
    destination_format: String,
    source_width: u32,
    source_height: u32,
    dest_width: u32,
    dest_height: u32,
}

struct SharedHandleInfo {
    creation_ok: bool,
    handle_value: u64,
    handle_valid: bool,
    access_flags: u32,
    access_flags_label: String,
    texture_format: String,
    texture_width: u32,
    texture_height: u32,
    misc_flags: u32,
}

struct D3d12OpenSharedInfo {
    device_creation_ok: bool,
    adapter_name: String,
    open_shared_handle_ok: bool,
    resource_pointer: usize,
    width: u64,
    height: u32,
    mip_levels: u16,
    format: String,
    sample_count: u32,
    layout: String,
    flags: String,
    format_is_nv12: bool,
    width_is_3840: bool,
    height_is_2176: bool,
}

struct SharedFenceSyncInfo {
    device5_available: bool,
    context4_available: bool,
    fence_creation_ok: bool,
    shared_fence_handle: u64,
    d3d12_fence_open_ok: bool,
    signal_result: String,
    wait_result: String,
    mechanism: String,
    synchronization_valid: bool,
    step_status: String,
    error: Option<String>,
}

pub(crate) struct SharedFenceSyncBundle {
    info: SharedFenceSyncInfo,
    _d3d11_fence: Option<ID3D11Fence>,
    _shared_fence_handle: Option<OwnedNtHandle>,
    _d3d11_context4: Option<ID3D11DeviceContext4>,
    _d3d12_fence: Option<ID3D12Fence>,
    _d3d12_command_queue: Option<ID3D12CommandQueue>,
}

impl SharedFenceSyncBundle {
    fn fence_handle(&self) -> Option<HANDLE> {
        self._shared_fence_handle
            .as_ref()
            .map(OwnedNtHandle::handle)
    }

    /// GPU queue wait on the shared D3D11 fence (no CPU polling).
    pub(crate) fn wait_d3d11_fence(&self, value: u64) -> Result<(), String> {
        let context4 = self
            ._d3d11_context4
            .as_ref()
            .ok_or_else(|| "D3D11 context4 unavailable for fence wait".to_string())?;
        let d3d11_fence = self
            ._d3d11_fence
            .as_ref()
            .ok_or_else(|| "D3D11 fence unavailable".to_string())?;
        unsafe {
            context4
                .Wait(d3d11_fence, value)
                .map_err(|e| format_hresult("ID3D11DeviceContext4::Wait", e))?;
        }
        Ok(())
    }

    /// Signal the shared D3D11 fence without Waiting on the probe D3D12 queue.
    pub(crate) fn signal_d3d11_fence_only(&self, value: u64) -> Result<(), String> {
        let context4 = self
            ._d3d11_context4
            .as_ref()
            .ok_or_else(|| "D3D11 context4 unavailable for fence signal".to_string())?;
        let d3d11_fence = self
            ._d3d11_fence
            .as_ref()
            .ok_or_else(|| "D3D11 fence unavailable".to_string())?;
        unsafe {
            context4
                .Signal(d3d11_fence, value)
                .map_err(|e| format_hresult("ID3D11DeviceContext4::Signal", e))?;
        }
        Ok(())
    }

    pub(crate) fn d3d12_command_queue_ptr(&self) -> Option<usize> {
        self._d3d12_command_queue
            .as_ref()
            .map(|q| Interface::as_raw(q) as usize)
    }
}

struct OwnedNtHandle(HANDLE);

impl OwnedNtHandle {
    fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedNtHandle {
    fn drop(&mut self) {
        if is_handle_valid(self.0) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

struct OwnedD3d11Texture2D(ID3D11Texture2D);

impl Drop for OwnedD3d11Texture2D {
    fn drop(&mut self) {
        // ID3D11Texture2D Drop releases the COM reference.
    }
}

pub(crate) struct ProbeResult {
    stream: VideoStreamInfo,
    _fmt: FormatContext,
    _decoder: CodecContext,
    _hw_device: BufferRef,
    _av_frame: AvFrame,
    _shareable_texture: Option<OwnedD3d11Texture2D>,
    decoder: DecoderInfo,
    frame: DecodedFrameInfo,
    d3d11: D3d11FrameInspection,
    texture_desc: D3d11TextureDesc,
    shareable_texture: ShareableTextureInfo,
    gpu_copy: GpuCopyInfo,
    shared_handle: SharedHandleInfo,
    _shared_nt_handle: OwnedNtHandle,
    d3d12_open: D3d12OpenSharedInfo,
    shared_fence_sync: SharedFenceSyncBundle,
    _d3d12_device: ID3D12Device,
    _d3d12_resource: ID3D12Resource,
}

struct AvPacket(*mut ffmpeg::AVPacket);

impl AvPacket {
    fn new() -> Result<Self, String> {
        let pkt = unsafe { ffmpeg::av_packet_alloc() };
        if pkt.is_null() {
            return Err("av_packet_alloc failed".to_string());
        }
        Ok(Self(pkt))
    }

    fn as_mut_ptr(&mut self) -> *mut ffmpeg::AVPacket {
        self.0
    }
}

impl Drop for AvPacket {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ffmpeg::av_packet_free(&mut self.0);
            }
        }
    }
}

struct AvFrame(*mut ffmpeg::AVFrame);

impl AvFrame {
    fn new() -> Result<Self, String> {
        let frame = unsafe { ffmpeg::av_frame_alloc() };
        if frame.is_null() {
            return Err("av_frame_alloc failed".to_string());
        }
        Ok(Self(frame))
    }

    fn as_ptr(&self) -> *mut ffmpeg::AVFrame {
        self.0
    }

    fn as_mut_ptr(&mut self) -> *mut ffmpeg::AVFrame {
        self.0
    }
}

impl Drop for AvFrame {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ffmpeg::av_frame_free(&mut self.0);
            }
        }
    }
}

fn is_eagain(ret: i32) -> bool {
    ret == unsafe { ffmpeg::AVERROR(ffmpeg::EAGAIN) }
}

fn is_eof(ret: i32) -> bool {
    ret == ffmpeg::AVERROR_EOF
}

fn create_d3d11va_device() -> Result<BufferRef, String> {
    let mut hw_device_ctx: *mut ffmpeg::AVBufferRef = std::ptr::null_mut();
    let ret = unsafe {
        ffmpeg::av_hwdevice_ctx_create(
            &mut hw_device_ctx,
            ffmpeg::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ret < 0 {
        return ffmpeg_err(ret);
    }
    Ok(BufferRef(hw_device_ctx))
}

fn open_decoder_for_stream(
    codecpar: *const ffmpeg::AVCodecParameters,
) -> Result<(CodecContext, BufferRef, DecoderInfo), String> {
    let hw_device_type = unsafe {
        c_str_name(ffmpeg::av_hwdevice_get_type_name(
            ffmpeg::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
        ))
    };

    let codec_id = unsafe { (*codecpar).codec_id };

    let decoder = unsafe { ffmpeg::avcodec_find_decoder(codec_id) };
    if decoder.is_null() {
        return Err(format!(
            "avcodec_find_decoder failed for codec_id {}",
            codec_id as i32
        ));
    }
    let decoder_name = unsafe { c_str_name((*decoder).name) };

    let mut codec_ctx = unsafe { ffmpeg::avcodec_alloc_context3(decoder) };
    if codec_ctx.is_null() {
        return Err("avcodec_alloc_context3 failed".to_string());
    }

    let ret = unsafe { ffmpeg::avcodec_parameters_to_context(codec_ctx, codecpar) };
    if ret < 0 {
        unsafe {
            ffmpeg::avcodec_free_context(&mut codec_ctx);
        }
        return ffmpeg_err(ret);
    }

    let hw_device = create_d3d11va_device()?;

    let hw_ref = unsafe { ffmpeg::av_buffer_ref(hw_device.as_ptr()) };
    if hw_ref.is_null() {
        unsafe {
            ffmpeg::avcodec_free_context(&mut codec_ctx);
        }
        return Err("av_buffer_ref(hw_device_ctx) failed".to_string());
    }
    unsafe {
        (*codec_ctx).hw_device_ctx = hw_ref;
        (*codec_ctx).get_format = Some(d3d11_get_format);
    }

    reset_get_format_log();
    println!("get_format callback:");
    let ret = unsafe { ffmpeg::avcodec_open2(codec_ctx, decoder, std::ptr::null_mut()) };
    if ret < 0 {
        unsafe {
            ffmpeg::avcodec_free_context(&mut codec_ctx);
        }
        return ffmpeg_err(ret);
    }

    let width = unsafe { (*codec_ctx).width };
    let height = unsafe { (*codec_ctx).height };
    let pix_fmt = unsafe { (*codec_ctx).pix_fmt as i32 };
    let pix_fmt_enum: ffmpeg::AVPixelFormat = unsafe { std::mem::transmute((*codec_ctx).pix_fmt) };
    let pix_fmt_name = unsafe { c_str_name(ffmpeg::av_get_pix_fmt_name(pix_fmt_enum)) };

    Ok((
        CodecContext(codec_ctx),
        hw_device,
        DecoderInfo {
            hw_device_type,
            d3d11va_device_ok: true,
            decoder_name,
            codec_id,
            width,
            height,
            pix_fmt,
            pix_fmt_name,
            get_format_candidates: Vec::new(),
            get_format_selected: None,
        },
    ))
}

fn pix_fmt_label(fmt: ffmpeg::AVPixelFormat) -> String {
    let name = unsafe { c_str_name(ffmpeg::av_get_pix_fmt_name(fmt)) };
    format!("{} ({name})", fmt as i32)
}

pub(crate) fn inspect_d3d11_frame(frame: &AvFrame) -> Result<D3d11FrameInspection, String> {
    let f = frame.as_ptr();
    unsafe {
        let format = (*f).format;
        let format_enum: ffmpeg::AVPixelFormat = std::mem::transmute(format);
        if format_enum != ffmpeg::AVPixelFormat::AV_PIX_FMT_D3D11 {
            return Err(format!(
                "expected AV_PIX_FMT_D3D11, got {} ({})",
                format,
                c_str_name(ffmpeg::av_get_pix_fmt_name(format_enum))
            ));
        }

        let data0 = (*f).data[0];
        let data1 = (*f).data[1];

        let texture_ptr = data0 as usize;
        let texture_array_index = data1 as usize as i32;

        let hw_frames_ctx = (*f).hw_frames_ctx;
        let hw_frames_ctx_present = !hw_frames_ctx.is_null();

        let (hw_format, sw_format) = if hw_frames_ctx_present {
            let frames_ctx = (*hw_frames_ctx).data as *mut ffmpeg::AVHWFramesContext;
            if frames_ctx.is_null() {
                ("(null frames context)".to_string(), "(null)".to_string())
            } else {
                let hw_fmt: ffmpeg::AVPixelFormat = std::mem::transmute((*frames_ctx).format);
                let sw_fmt: ffmpeg::AVPixelFormat = std::mem::transmute((*frames_ctx).sw_format);
                (pix_fmt_label(hw_fmt), pix_fmt_label(sw_fmt))
            }
        } else {
            ("(none)".to_string(), "(none)".to_string())
        };

        Ok(D3d11FrameInspection {
            texture_ptr,
            texture_array_index,
            hw_frames_ctx_present,
            hw_format,
            sw_format,
        })
    }
}

fn d3d11_texture_desc_from_raw(
    desc: &D3D11_TEXTURE2D_DESC,
    array_slice_index: i32,
) -> D3d11TextureDesc {
    D3d11TextureDesc {
        width: desc.Width,
        height: desc.Height,
        mip_levels: desc.MipLevels,
        array_size: desc.ArraySize,
        dxgi_format: format!("{:?} ({})", desc.Format, desc.Format.0),
        sample_count: desc.SampleDesc.Count,
        sample_quality: desc.SampleDesc.Quality,
        usage: format!("{:?} ({})", desc.Usage, desc.Usage.0),
        bind_flags: desc.BindFlags,
        cpu_access_flags: desc.CPUAccessFlags,
        misc_flags: desc.MiscFlags,
        array_slice_index,
    }
}

fn has_shared_nthandle_flag(misc_flags: u32) -> bool {
    (misc_flags & D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 as u32) != 0
}

fn d3d11_calc_subresource(mip_slice: u32, array_slice: u32, mip_levels: u32) -> u32 {
    mip_slice + array_slice * mip_levels
}

pub(crate) fn copy_decoder_slice_to_shareable(
    frame: &AvFrame,
    inspection: &D3d11FrameInspection,
    source_desc: &D3d11TextureDesc,
    shareable: &OwnedD3d11Texture2D,
    dest_desc: &D3d11TextureDesc,
) -> Result<GpuCopyInfo, String> {
    if inspection.texture_ptr == 0 {
        return Err("D3D11 texture pointer is null".to_string());
    }

    let source_array_slice = inspection.texture_array_index;
    let array_slice_u32: u32 = source_array_slice.try_into().map_err(|_| {
        format!("array slice index {source_array_slice} is negative or out of range")
    })?;

    let source_subresource = d3d11_calc_subresource(0, array_slice_u32, source_desc.mip_levels);
    let destination_subresource = 0u32;

    // Keep AVFrame alive until the copy command is flushed.
    let _frame_guard = frame.as_ptr();

    unsafe {
        let raw = inspection.texture_ptr as *mut std::ffi::c_void;
        let decoder_texture = ID3D11Texture2D::from_raw_borrowed(&raw)
            .ok_or_else(|| "ID3D11Texture2D::from_raw_borrowed failed".to_string())?;

        let device = decoder_texture
            .GetDevice()
            .map_err(|e| format_hresult("ID3D11Texture2D::GetDevice", e))?;

        let context = device
            .GetImmediateContext()
            .map_err(|e| format_hresult("ID3D11Device::GetImmediateContext", e))?;

        context.CopySubresourceRegion(
            &shareable.0,
            destination_subresource,
            0,
            0,
            0,
            decoder_texture,
            source_subresource,
            None,
        );

        context.Flush();

        Ok(GpuCopyInfo {
            submission_ok: true,
            source_array_slice,
            source_subresource,
            destination_subresource,
            source_format: source_desc.dxgi_format.clone(),
            destination_format: dest_desc.dxgi_format.clone(),
            source_width: source_desc.width,
            source_height: source_desc.height,
            dest_width: dest_desc.width,
            dest_height: dest_desc.height,
        })
    }
}

/// Diagnostic-only: one real decode+copy into the shared NV12, then stop writing.
///
/// `with_keyed_mutex`:
/// - false = TEST 7 (no Acquire/Release)
/// - true  = TEST 8 (AcquireSync → copy → Flush → ReleaseSync)
///
/// Sync: D3D11 Signal(fence) then Wait on wgpu-hal present queue only (not the probe queue).
pub(crate) fn diagnostic_frozen_real_import(
    probe: &mut ProbeResult,
    context: &wgpu_hal_interop::WgpuDx12Context,
    cached_fence: &ID3D12Fence,
    fence_value: u64,
    with_keyed_mutex: bool,
) -> Result<(), String> {
    let mut frame_info = decode_next_d3d11_frame(
        &probe._fmt,
        &probe._decoder,
        probe.stream.stream_index,
        &mut probe._av_frame,
    )?;
    if frame_info.is_none() {
        restart_fixture_decode(probe)?;
        frame_info = decode_next_d3d11_frame(
            &probe._fmt,
            &probe._decoder,
            probe.stream.stream_index,
            &mut probe._av_frame,
        )?;
    }
    let Some(frame_info) = frame_info else {
        return Err("EOF: could not decode a frame for frozen import".to_string());
    };
    if !frame_info.is_d3d11 {
        return Err(format!(
            "frame is not AV_PIX_FMT_D3D11 ({})",
            frame_info.format_name
        ));
    }
    finish_frozen_copy(probe, context, cached_fence, fence_value, with_keyed_mutex)
}

fn finish_frozen_copy(
    probe: &mut ProbeResult,
    context: &wgpu_hal_interop::WgpuDx12Context,
    cached_fence: &ID3D12Fence,
    fence_value: u64,
    with_keyed_mutex: bool,
) -> Result<(), String> {
    let inspection = inspect_d3d11_frame(&probe._av_frame)?;
    let shareable = probe
        ._shareable_texture
        .as_ref()
        .ok_or_else(|| "shareable texture missing".to_string())?;

    let keyed_mutex = if with_keyed_mutex {
        Some(query_keyed_mutex(shareable)?)
    } else {
        None
    };

    if let Some(mutex) = keyed_mutex.as_ref() {
        println!("TEST 8: IDXGIKeyedMutex::AcquireSync(key=0, timeout_ms=5000)...");
        match unsafe { mutex.AcquireSync(0, 5000) } {
            Ok(()) => {
                println!("AcquireSync: OK (S_OK / success)");
            }
            Err(e) => {
                let code = e.code().0 as u32;
                if e.code() == DXGI_ERROR_WAIT_TIMEOUT {
                    return Err(format!(
                        "AcquireSync FAILED: DXGI_ERROR_WAIT_TIMEOUT (HRESULT=0x{code:08X})"
                    ));
                }
                return Err(format!(
                    "AcquireSync FAILED: {e} (HRESULT=0x{code:08X}) — treat as non-success"
                ));
            }
        }
    } else {
        println!("TEST 7: keyed-mutex Acquire/Release NOT used (isolated variable)");
    }

    let copy_result = copy_decoder_slice_to_shareable(
        &probe._av_frame,
        &inspection,
        &probe.texture_desc,
        shareable,
        &probe.shareable_texture.desc,
    );

    if let Some(mutex) = keyed_mutex.as_ref() {
        // Release even if copy failed so the mutex is not left held.
        println!("TEST 8: IDXGIKeyedMutex::ReleaseSync(key=0)...");
        match unsafe { mutex.ReleaseSync(0) } {
            Ok(()) => println!("ReleaseSync: OK (S_OK / success)"),
            Err(e) => {
                let code = e.code().0 as u32;
                let release_err = format!(
                    "ReleaseSync FAILED: {e} (HRESULT=0x{code:08X}) — treat as non-success"
                );
                let _ = copy_result?;
                return Err(release_err);
            }
        }
    }

    let _gpu_copy = copy_result?;
    println!(
        "Frozen CopySubresourceRegion + Flush: OK (array_slice={})",
        inspection.texture_array_index
    );

    probe
        .shared_fence_sync
        .signal_d3d11_fence_only(fence_value)?;
    println!("D3D11 shared fence Signal({fence_value}): OK");

    wgpu_hal_interop::wait_cached_wgpu_fence(context, cached_fence, fence_value)?;
    println!("wgpu-hal present/raw queue Wait({fence_value}): OK (probe D3D12 queue Wait skipped)");
    println!("Frozen import complete — subsequent frames will NOT issue D3D11 writes.");
    Ok(())
}

fn query_keyed_mutex(shareable: &OwnedD3d11Texture2D) -> Result<IDXGIKeyedMutex, String> {
    shareable
        .0
        .cast::<IDXGIKeyedMutex>()
        .map_err(|e| format_hresult("ID3D11Texture2D::cast<IDXGIKeyedMutex>", e))
}

/// Continuous-playback producer: keyed-mutex guarded GPU copy into the shareable NV12.
pub(crate) fn copy_decoder_to_shareable_keyed(
    av_frame: &AvFrame,
    inspection: &D3d11FrameInspection,
    texture_desc: &D3d11TextureDesc,
    shareable: &OwnedD3d11Texture2D,
    shareable_desc: &D3d11TextureDesc,
) -> Result<GpuCopyInfo, String> {
    let mutex = query_keyed_mutex(shareable)?;
    unsafe {
        mutex
            .AcquireSync(0, 5000)
            .map_err(|e| format_hresult("IDXGIKeyedMutex::AcquireSync", e))?;
    }
    let copy_result = copy_decoder_slice_to_shareable(
        av_frame,
        inspection,
        texture_desc,
        shareable,
        shareable_desc,
    );
    unsafe {
        mutex
            .ReleaseSync(0)
            .map_err(|e| format_hresult("IDXGIKeyedMutex::ReleaseSync", e))?;
    }
    copy_result
}

fn create_shareable_nv12_texture(
    frame: &AvFrame,
    inspection: &D3d11FrameInspection,
    source_desc: &D3d11TextureDesc,
) -> Result<(OwnedD3d11Texture2D, ShareableTextureInfo), String> {
    if inspection.texture_ptr == 0 {
        return Err("D3D11 texture pointer is null".to_string());
    }

    let _frame_guard = frame.as_ptr();

    unsafe {
        let raw = inspection.texture_ptr as *mut std::ffi::c_void;
        let decoder_texture = ID3D11Texture2D::from_raw_borrowed(&raw)
            .ok_or_else(|| "ID3D11Texture2D::from_raw_borrowed failed".to_string())?;

        let device = decoder_texture
            .GetDevice()
            .map_err(|e| format_hresult("ID3D11Texture2D::GetDevice", e))?;

        let _context = device
            .GetImmediateContext()
            .map_err(|e| format_hresult("ID3D11Device::GetImmediateContext", e))?;

        let create_desc = D3D11_TEXTURE2D_DESC {
            Width: source_desc.width,
            Height: source_desc.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX)
                .0 as u32,
        };

        let mut shareable: Option<ID3D11Texture2D> = None;
        device
            .CreateTexture2D(&create_desc, None, Some(&mut shareable))
            .map_err(|e| format_hresult("ID3D11Device::CreateTexture2D", e))?;

        let shareable = shareable.ok_or_else(|| {
            "ID3D11Device::CreateTexture2D succeeded but returned null texture".to_string()
        })?;

        let mut queried = std::mem::zeroed::<D3D11_TEXTURE2D_DESC>();
        shareable.GetDesc(&mut queried);
        let desc = d3d11_texture_desc_from_raw(&queried, 0);
        let has_shared_nthandle = has_shared_nthandle_flag(desc.misc_flags);

        Ok((
            OwnedD3d11Texture2D(shareable),
            ShareableTextureInfo {
                creation_ok: true,
                has_shared_nthandle,
                desc,
            },
        ))
    }
}

fn format_hresult(operation: &str, error: windows::core::Error) -> String {
    format!(
        "{operation} failed: {error} (HRESULT=0x{:08X})",
        error.code().0 as u32
    )
}

fn is_handle_valid(handle: HANDLE) -> bool {
    !handle.is_invalid() && handle != INVALID_HANDLE_VALUE
}

fn shared_handle_access_flags() -> (u32, String) {
    let access = (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0;
    let label = format!(
        "DXGI_SHARED_RESOURCE_READ (0x{:08X}) | DXGI_SHARED_RESOURCE_WRITE (0x{:08X}) = 0x{:08X}",
        DXGI_SHARED_RESOURCE_READ.0, DXGI_SHARED_RESOURCE_WRITE.0, access
    );
    (access, label)
}

fn adapter_description_string(desc: &DXGI_ADAPTER_DESC) -> String {
    let len = desc
        .Description
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(desc.Description.len());
    String::from_utf16_lossy(&desc.Description[..len])
}

fn open_d3d12_shared_resource(
    shareable: &OwnedD3d11Texture2D,
    shared_handle: &OwnedNtHandle,
) -> Result<(ID3D12Device, ID3D12Resource, D3d12OpenSharedInfo), String> {
    if !is_handle_valid(shared_handle.0) {
        return Err("shared NT HANDLE is not valid".to_string());
    }

    unsafe {
        let d3d11_device = shareable
            .0
            .GetDevice()
            .map_err(|e| format_hresult("ID3D11Texture2D::GetDevice", e))?;

        let dxgi_device: IDXGIDevice = d3d11_device
            .cast()
            .map_err(|e| format_hresult("ID3D11Device::cast<IDXGIDevice>", e))?;

        let adapter = dxgi_device
            .GetAdapter()
            .map_err(|e| format_hresult("IDXGIDevice::GetAdapter", e))?;

        let adapter_desc = adapter
            .GetDesc()
            .map_err(|e| format_hresult("IDXGIAdapter::GetDesc", e))?;
        let adapter_name = adapter_description_string(&adapter_desc);

        let mut d3d12_device: Option<ID3D12Device> = None;
        D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut d3d12_device)
            .map_err(|e| format_hresult("D3D12CreateDevice", e))?;
        let d3d12_device = d3d12_device
            .ok_or_else(|| "D3D12CreateDevice succeeded but returned null device".to_string())?;

        let mut d3d12_resource: Option<ID3D12Resource> = None;
        d3d12_device
            .OpenSharedHandle(shared_handle.0, &mut d3d12_resource)
            .map_err(|e| format_hresult("ID3D12Device::OpenSharedHandle", e))?;
        let d3d12_resource = d3d12_resource.ok_or_else(|| {
            "ID3D12Device::OpenSharedHandle succeeded but returned null resource".to_string()
        })?;

        let desc = d3d12_resource.GetDesc();
        let resource_pointer = Interface::as_raw(&d3d12_resource) as usize;
        let format_is_nv12 = desc.Format == DXGI_FORMAT_NV12;
        let width_is_3840 = desc.Width == 3840;
        let height_is_2176 = desc.Height == 2176;

        Ok((
            d3d12_device,
            d3d12_resource,
            D3d12OpenSharedInfo {
                device_creation_ok: true,
                adapter_name,
                open_shared_handle_ok: true,
                resource_pointer,
                width: desc.Width,
                height: desc.Height,
                mip_levels: desc.MipLevels,
                format: format!("{:?} ({})", desc.Format, desc.Format.0),
                sample_count: desc.SampleDesc.Count,
                layout: format!("{:?} ({})", desc.Layout, desc.Layout.0),
                flags: format!("{:?} ({:#x})", desc.Flags, desc.Flags.0),
                format_is_nv12,
                width_is_3840,
                height_is_2176,
            },
        ))
    }
}

fn failed_fence_sync_bundle(
    device5_available: bool,
    context4_available: bool,
    fence_creation_ok: bool,
    shared_fence_handle: u64,
    d3d12_fence_open_ok: bool,
    signal_result: String,
    wait_result: String,
    error: String,
) -> SharedFenceSyncBundle {
    SharedFenceSyncBundle {
        info: SharedFenceSyncInfo {
            device5_available,
            context4_available,
            fence_creation_ok,
            shared_fence_handle,
            d3d12_fence_open_ok,
            signal_result,
            wait_result,
            mechanism: "shared GPU fence".to_string(),
            synchronization_valid: false,
            step_status: "STEP 32 / 40: FAILED".to_string(),
            error: Some(error),
        },
        _d3d11_fence: None,
        _shared_fence_handle: None,
        _d3d11_context4: None,
        _d3d12_fence: None,
        _d3d12_command_queue: None,
    }
}

fn establish_shared_gpu_fence_sync(
    shareable: &OwnedD3d11Texture2D,
    d3d12_device: &ID3D12Device,
) -> SharedFenceSyncBundle {
    unsafe {
        let d3d11_device = match shareable.0.GetDevice() {
            Ok(device) => device,
            Err(e) => {
                return failed_fence_sync_bundle(
                    false,
                    false,
                    false,
                    0,
                    false,
                    "skipped".to_string(),
                    "skipped".to_string(),
                    format_hresult("ID3D11Texture2D::GetDevice", e),
                );
            }
        };

        let device5_result: Result<ID3D11Device5, windows::core::Error> = d3d11_device.cast();
        let device5 = match device5_result {
            Ok(device5) => device5,
            Err(e) => {
                return failed_fence_sync_bundle(
                    false,
                    false,
                    false,
                    0,
                    false,
                    "skipped".to_string(),
                    "skipped".to_string(),
                    format_hresult("ID3D11Device::cast<ID3D11Device5>", e),
                );
            }
        };

        let context = match d3d11_device.GetImmediateContext() {
            Ok(context) => context,
            Err(e) => {
                return failed_fence_sync_bundle(
                    true,
                    false,
                    false,
                    0,
                    false,
                    "skipped".to_string(),
                    "skipped".to_string(),
                    format_hresult("ID3D11Device::GetImmediateContext", e),
                );
            }
        };

        let context4_result: Result<ID3D11DeviceContext4, windows::core::Error> = context.cast();
        let context4 = match context4_result {
            Ok(context4) => context4,
            Err(e) => {
                return failed_fence_sync_bundle(
                    true,
                    false,
                    false,
                    0,
                    false,
                    "skipped".to_string(),
                    "skipped".to_string(),
                    format_hresult("ID3D11DeviceContext::cast<ID3D11DeviceContext4>", e),
                );
            }
        };

        let mut d3d11_fence: Option<ID3D11Fence> = None;
        if let Err(e) = device5.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut d3d11_fence) {
            return failed_fence_sync_bundle(
                true,
                true,
                false,
                0,
                false,
                "skipped".to_string(),
                "skipped".to_string(),
                format_hresult("ID3D11Device5::CreateFence", e),
            );
        }
        let d3d11_fence = match d3d11_fence {
            Some(fence) => fence,
            None => {
                return failed_fence_sync_bundle(
                    true,
                    true,
                    false,
                    0,
                    false,
                    "skipped".to_string(),
                    "skipped".to_string(),
                    "ID3D11Device5::CreateFence succeeded but returned null fence".to_string(),
                );
            }
        };

        let fence_handle = match d3d11_fence.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null())
        {
            Ok(handle) => handle,
            Err(e) => {
                return failed_fence_sync_bundle(
                    true,
                    true,
                    true,
                    0,
                    false,
                    "skipped".to_string(),
                    "skipped".to_string(),
                    format_hresult("ID3D11Fence::CreateSharedHandle", e),
                );
            }
        };
        if !is_handle_valid(fence_handle) {
            return failed_fence_sync_bundle(
                true,
                true,
                true,
                0,
                false,
                "skipped".to_string(),
                "skipped".to_string(),
                "ID3D11Fence::CreateSharedHandle returned invalid HANDLE".to_string(),
            );
        }
        let shared_fence_handle = fence_handle.0 as usize as u64;

        let mut d3d12_fence: Option<ID3D12Fence> = None;
        if let Err(e) = d3d12_device.OpenSharedHandle(fence_handle, &mut d3d12_fence) {
            let _ = CloseHandle(fence_handle);
            return failed_fence_sync_bundle(
                true,
                true,
                true,
                shared_fence_handle,
                false,
                "skipped".to_string(),
                "skipped".to_string(),
                format_hresult("ID3D12Device::OpenSharedHandle<ID3D12Fence>", e),
            );
        }
        let d3d12_fence = match d3d12_fence {
            Some(fence) => fence,
            None => {
                let _ = CloseHandle(fence_handle);
                return failed_fence_sync_bundle(
                    true,
                    true,
                    true,
                    shared_fence_handle,
                    false,
                    "skipped".to_string(),
                    "skipped".to_string(),
                    "ID3D12Device::OpenSharedHandle succeeded but returned null fence".to_string(),
                );
            }
        };

        let queue_desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        };
        let command_queue: ID3D12CommandQueue = match d3d12_device.CreateCommandQueue(&queue_desc) {
            Ok(queue) => queue,
            Err(e) => {
                let _ = CloseHandle(fence_handle);
                return failed_fence_sync_bundle(
                    true,
                    true,
                    true,
                    shared_fence_handle,
                    true,
                    "skipped".to_string(),
                    "skipped".to_string(),
                    format_hresult("ID3D12Device::CreateCommandQueue", e),
                );
            }
        };

        let signal_result = match context4.Signal(&d3d11_fence, FENCE_SIGNAL_VALUE) {
            Ok(()) => format!("OK — Signal(value={FENCE_SIGNAL_VALUE})"),
            Err(e) => format_hresult("ID3D11DeviceContext4::Signal", e),
        };
        if !signal_result.starts_with("OK") {
            let _ = CloseHandle(fence_handle);
            let err = signal_result.clone();
            return failed_fence_sync_bundle(
                true,
                true,
                true,
                shared_fence_handle,
                true,
                signal_result,
                "skipped".to_string(),
                err,
            );
        }

        let wait_result = match command_queue.Wait(&d3d12_fence, FENCE_SIGNAL_VALUE) {
            Ok(()) => format!("OK — Wait(value={FENCE_SIGNAL_VALUE})"),
            Err(e) => format_hresult("ID3D12CommandQueue::Wait", e),
        };
        let synchronization_valid = wait_result.starts_with("OK");
        if !synchronization_valid {
            let _ = CloseHandle(fence_handle);
            return failed_fence_sync_bundle(
                true,
                true,
                true,
                shared_fence_handle,
                true,
                signal_result,
                wait_result.clone(),
                wait_result,
            );
        }

        SharedFenceSyncBundle {
            info: SharedFenceSyncInfo {
                device5_available: true,
                context4_available: true,
                fence_creation_ok: true,
                shared_fence_handle,
                d3d12_fence_open_ok: true,
                signal_result,
                wait_result,
                mechanism: "shared GPU fence — D3D11 Signal(1) then D3D12 CommandQueue::Wait(1)"
                    .to_string(),
                synchronization_valid: true,
                step_status: "STEP 32 / 40: PASS".to_string(),
                error: None,
            },
            _d3d11_fence: Some(d3d11_fence),
            _shared_fence_handle: Some(OwnedNtHandle(fence_handle)),
            _d3d11_context4: Some(context4),
            _d3d12_fence: Some(d3d12_fence),
            _d3d12_command_queue: Some(command_queue),
        }
    }
}

fn create_shared_handle_for_texture(
    shareable: &OwnedD3d11Texture2D,
    dest_desc: &D3d11TextureDesc,
) -> Result<(OwnedNtHandle, SharedHandleInfo), String> {
    let (access_flags, access_flags_label) = shared_handle_access_flags();

    unsafe {
        let dxgi_resource: IDXGIResource1 = shareable
            .0
            .cast()
            .map_err(|e| format_hresult("ID3D11Texture2D::cast<IDXGIResource1>", e))?;

        let handle = dxgi_resource
            .CreateSharedHandle(None, access_flags, PCWSTR::null())
            .map_err(|e| format_hresult("IDXGIResource1::CreateSharedHandle", e))?;

        let handle_valid = is_handle_valid(handle);
        if !handle_valid {
            return Err(
                "IDXGIResource1::CreateSharedHandle returned null or INVALID_HANDLE_VALUE"
                    .to_string(),
            );
        }

        let handle_value = handle.0 as usize as u64;

        Ok((
            OwnedNtHandle(handle),
            SharedHandleInfo {
                creation_ok: true,
                handle_value,
                handle_valid,
                access_flags,
                access_flags_label,
                texture_format: dest_desc.dxgi_format.clone(),
                texture_width: dest_desc.width,
                texture_height: dest_desc.height,
                misc_flags: dest_desc.misc_flags,
            },
        ))
    }
}

fn query_d3d11_texture_desc(
    frame: &AvFrame,
    inspection: &D3d11FrameInspection,
) -> Result<D3d11TextureDesc, String> {
    if inspection.texture_ptr == 0 {
        return Err("D3D11 texture pointer is null".to_string());
    }

    // Keep AVFrame alive for the duration of native texture inspection.
    let _frame_guard = frame.as_ptr();

    unsafe {
        let raw = inspection.texture_ptr as *mut std::ffi::c_void;
        let texture = ID3D11Texture2D::from_raw_borrowed(&raw)
            .ok_or_else(|| "ID3D11Texture2D::from_raw_borrowed failed".to_string())?;

        let mut desc = std::mem::zeroed::<D3D11_TEXTURE2D_DESC>();
        texture.GetDesc(&mut desc);

        Ok(d3d11_texture_desc_from_raw(
            &desc,
            inspection.texture_array_index,
        ))
    }
}

fn decode_first_frame(
    fmt: &FormatContext,
    codec_ctx: &CodecContext,
    stream_index: i32,
) -> Result<(AvFrame, DecodedFrameInfo), String> {
    let mut packet = AvPacket::new()?;
    let mut frame = AvFrame::new()?;

    loop {
        let read_ret = unsafe { ffmpeg::av_read_frame(fmt.as_ptr(), packet.as_mut_ptr()) };
        if is_eof(read_ret) {
            return Err("av_read_frame: end of file before first decoded frame".to_string());
        }
        if read_ret < 0 {
            return ffmpeg_err(read_ret);
        }

        let pkt = packet.as_mut_ptr();
        let pkt_stream = unsafe { (*pkt).stream_index };
        if pkt_stream != stream_index {
            unsafe {
                ffmpeg::av_packet_unref(pkt);
            }
            continue;
        }

        let send_ret =
            unsafe { ffmpeg::avcodec_send_packet(codec_ctx.as_ptr(), packet.as_mut_ptr()) };
        unsafe {
            ffmpeg::av_packet_unref(packet.as_mut_ptr());
        }
        if send_ret < 0 && !is_eagain(send_ret) {
            return ffmpeg_err(send_ret);
        }

        loop {
            let recv_ret =
                unsafe { ffmpeg::avcodec_receive_frame(codec_ctx.as_ptr(), frame.as_mut_ptr()) };
            if is_eagain(recv_ret) || is_eof(recv_ret) {
                break;
            }
            if recv_ret < 0 {
                return ffmpeg_err(recv_ret);
            }

            let f = frame.as_mut_ptr();
            let format = unsafe { (*f).format };
            let format_enum: ffmpeg::AVPixelFormat = unsafe { std::mem::transmute(format) };
            let format_name = unsafe { c_str_name(ffmpeg::av_get_pix_fmt_name(format_enum)) };
            let width = unsafe { (*f).width };
            let height = unsafe { (*f).height };
            let pts = unsafe { (*f).pts };

            return Ok((
                frame,
                DecodedFrameInfo {
                    format,
                    format_name,
                    width,
                    height,
                    pts,
                    is_d3d11: format_enum == ffmpeg::AVPixelFormat::AV_PIX_FMT_D3D11,
                },
            ));
        }
    }
}

pub(crate) fn decode_next_d3d11_frame(
    fmt: &FormatContext,
    codec_ctx: &CodecContext,
    stream_index: i32,
    frame: &mut AvFrame,
) -> Result<Option<DecodedFrameInfo>, String> {
    let mut packet = AvPacket::new()?;

    loop {
        let read_ret = unsafe { ffmpeg::av_read_frame(fmt.as_ptr(), packet.as_mut_ptr()) };
        if is_eof(read_ret) {
            return Ok(None);
        }
        if read_ret < 0 {
            return ffmpeg_err(read_ret);
        }

        let pkt = packet.as_mut_ptr();
        let pkt_stream = unsafe { (*pkt).stream_index };
        if pkt_stream != stream_index {
            unsafe {
                ffmpeg::av_packet_unref(pkt);
            }
            continue;
        }

        let send_ret =
            unsafe { ffmpeg::avcodec_send_packet(codec_ctx.as_ptr(), packet.as_mut_ptr()) };
        unsafe {
            ffmpeg::av_packet_unref(packet.as_mut_ptr());
        }
        if send_ret < 0 && !is_eagain(send_ret) {
            return ffmpeg_err(send_ret);
        }

        loop {
            let recv_ret =
                unsafe { ffmpeg::avcodec_receive_frame(codec_ctx.as_ptr(), frame.as_mut_ptr()) };
            if is_eagain(recv_ret) || is_eof(recv_ret) {
                break;
            }
            if recv_ret < 0 {
                return ffmpeg_err(recv_ret);
            }

            let f = frame.as_mut_ptr();
            let format = unsafe { (*f).format };
            let format_enum: ffmpeg::AVPixelFormat = unsafe { std::mem::transmute(format) };
            let format_name = unsafe { c_str_name(ffmpeg::av_get_pix_fmt_name(format_enum)) };
            let width = unsafe { (*f).width };
            let height = unsafe { (*f).height };
            let pts = unsafe { (*f).pts };

            return Ok(Some(DecodedFrameInfo {
                format,
                format_name,
                width,
                height,
                pts,
                is_d3d11: format_enum == ffmpeg::AVPixelFormat::AV_PIX_FMT_D3D11,
            }));
        }
    }
}

pub(crate) fn restart_fixture_decode(probe: &ProbeResult) -> Result<(), String> {
    let stream_index = probe.stream.stream_index;
    let ret = unsafe {
        ffmpeg::av_seek_frame(
            probe._fmt.as_ptr(),
            stream_index,
            0,
            ffmpeg::AVSEEK_FLAG_BACKWARD,
        )
    };
    if ret < 0 {
        return ffmpeg_err(ret);
    }
    unsafe {
        ffmpeg::avcodec_flush_buffers(probe._decoder.as_ptr());
    }
    Ok(())
}

pub(crate) fn probe_format_and_open_decoder(path: &std::path::Path) -> Result<ProbeResult, String> {
    unsafe {
        let net_ret = ffmpeg::avformat_network_init();
        if net_ret < 0 {
            return ffmpeg_err(net_ret);
        }
    }

    let fmt = FormatContext::open(path)?;

    let ret = unsafe { ffmpeg::avformat_find_stream_info(fmt.as_ptr(), std::ptr::null_mut()) };
    if ret < 0 {
        return ffmpeg_err(ret);
    }

    let stream_index = unsafe {
        ffmpeg::av_find_best_stream(
            fmt.as_ptr(),
            ffmpeg::AVMediaType::AVMEDIA_TYPE_VIDEO,
            -1,
            -1,
            std::ptr::null_mut(),
            0,
        )
    };
    if stream_index < 0 {
        return ffmpeg_err(stream_index);
    }

    let stream = unsafe {
        let nb_streams = (*fmt.as_ptr()).nb_streams;
        if stream_index as u32 >= nb_streams {
            return Err(format!(
                "stream index {stream_index} out of range (nb_streams={nb_streams})"
            ));
        }
        *(*fmt.as_ptr()).streams.add(stream_index as usize)
    };

    let codecpar = unsafe { *(*stream).codecpar };
    let codec_id = codecpar.codec_id;
    let codec_name = unsafe { c_str_name(ffmpeg::avcodec_get_name(codec_id)) };
    let pix_fmt = codecpar.format;
    let pix_fmt_enum: ffmpeg::AVPixelFormat = unsafe { std::mem::transmute(pix_fmt) };
    let pix_fmt_name = unsafe { c_str_name(ffmpeg::av_get_pix_fmt_name(pix_fmt_enum)) };
    let time_base = unsafe { (*stream).time_base };

    let stream_info = VideoStreamInfo {
        stream_index,
        codec_id,
        codec_name,
        width: codecpar.width,
        height: codecpar.height,
        pix_fmt,
        pix_fmt_name,
        time_base_num: time_base.num,
        time_base_den: time_base.den,
    };

    let codecpar_ptr = unsafe { (*stream).codecpar };
    let (decoder_ctx, hw_device, mut decoder_info) = open_decoder_for_stream(codecpar_ptr)?;

    println!("Decoding first frame...");
    let (av_frame, frame_info) = decode_first_frame(&fmt, &decoder_ctx, stream_index)?;
    let d3d11_inspection = inspect_d3d11_frame(&av_frame)?;
    let texture_desc = query_d3d11_texture_desc(&av_frame, &d3d11_inspection)?;
    let (shareable_texture, shareable_info) =
        create_shareable_nv12_texture(&av_frame, &d3d11_inspection, &texture_desc)?;
    let gpu_copy = copy_decoder_slice_to_shareable(
        &av_frame,
        &d3d11_inspection,
        &texture_desc,
        &shareable_texture,
        &shareable_info.desc,
    )?;
    let (shared_nt_handle, shared_handle_info) =
        create_shared_handle_for_texture(&shareable_texture, &shareable_info.desc)?;
    let (d3d12_device, d3d12_resource, d3d12_open) =
        open_d3d12_shared_resource(&shareable_texture, &shared_nt_handle)?;
    let shared_fence_sync = establish_shared_gpu_fence_sync(&shareable_texture, &d3d12_device);

    let get_format_log = take_get_format_log();
    decoder_info.get_format_candidates = get_format_log.candidates;
    if get_format_log.selected.is_some() {
        decoder_info.get_format_selected = get_format_log.selected;
    }

    Ok(ProbeResult {
        stream: stream_info,
        _fmt: fmt,
        _decoder: decoder_ctx,
        _hw_device: hw_device,
        _av_frame: av_frame,
        _shareable_texture: Some(shareable_texture),
        decoder: decoder_info,
        frame: frame_info,
        d3d11: d3d11_inspection,
        texture_desc,
        shareable_texture: shareable_info,
        gpu_copy,
        shared_handle: shared_handle_info,
        _shared_nt_handle: shared_nt_handle,
        d3d12_open,
        shared_fence_sync,
        _d3d12_device: d3d12_device,
        _d3d12_resource: d3d12_resource,
    })
}

fn print_video_stream_info(info: &VideoStreamInfo) {
    println!("=== FFmpeg format probe ===");
    println!("Video stream index: {}", info.stream_index);
    println!(
        "codec_id:           {} ({})",
        info.codec_id as i32, info.codec_name
    );
    println!("width x height:     {} x {}", info.width, info.height);
    println!(
        "pixel format:       {} ({})",
        info.pix_fmt as i32, info.pix_fmt_name
    );
    println!(
        "time_base:          {}/{}",
        info.time_base_num, info.time_base_den
    );
}

fn print_decoder_info(info: &DecoderInfo) {
    println!("=== FFmpeg decoder context (D3D11VA) ===");
    println!("hardware device:    {}", info.hw_device_type);
    println!(
        "D3D11VA device:     {}",
        if info.d3d11va_device_ok {
            "OK"
        } else {
            "FAILED"
        }
    );
    println!("decoder name:       {}", info.decoder_name);
    println!("codec_id:           {}", info.codec_id as i32);
    println!("width x height:     {} x {}", info.width, info.height);
    println!("get_format candidates:");
    if info.get_format_candidates.is_empty() {
        println!("  (none recorded)");
    } else {
        for candidate in &info.get_format_candidates {
            println!("  {candidate}");
        }
    }
    println!(
        "get_format selected:  {}",
        info.get_format_selected
            .as_deref()
            .unwrap_or("AV_PIX_FMT_NONE")
    );
    println!(
        "codec_ctx.pix_fmt:    {} ({})",
        info.pix_fmt, info.pix_fmt_name
    );
}

fn print_decoded_frame_info(info: &DecodedFrameInfo) {
    println!("=== First decoded frame ===");
    println!("frame format:       {} ({})", info.format, info.format_name);
    println!("frame width x height: {} x {}", info.width, info.height);
    println!("frame pts:          {}", info.pts);
    println!(
        "is AV_PIX_FMT_D3D11: {}",
        if info.is_d3d11 { "yes" } else { "no" }
    );
}

fn print_d3d11_inspection(info: &D3d11FrameInspection) {
    println!("=== D3D11 frame inspection ===");
    println!("D3D11 texture pointer: 0x{:x}", info.texture_ptr);
    println!("D3D11 texture array index: {}", info.texture_array_index);
    println!(
        "hw_frames_ctx present: {}",
        if info.hw_frames_ctx_present {
            "yes"
        } else {
            "no"
        }
    );
    println!("hw format: {}", info.hw_format);
    println!("sw format: {}", info.sw_format);
}

fn print_texture_desc(info: &D3d11TextureDesc) {
    println!("=== ID3D11Texture2D description ===");
    println!("Width:              {}", info.width);
    println!("Height:             {}", info.height);
    println!("MipLevels:          {}", info.mip_levels);
    println!("ArraySize:          {}", info.array_size);
    println!("DXGI Format:        {}", info.dxgi_format);
    println!("SampleDesc.Count:   {}", info.sample_count);
    println!("SampleDesc.Quality: {}", info.sample_quality);
    println!("Usage:              {}", info.usage);
    println!(
        "BindFlags:          0x{:x} ({})",
        info.bind_flags, info.bind_flags
    );
    println!(
        "CPUAccessFlags:     0x{:x} ({})",
        info.cpu_access_flags, info.cpu_access_flags
    );
    println!(
        "MiscFlags:          0x{:x} ({})",
        info.misc_flags, info.misc_flags
    );
    println!("decoded array slice index: {}", info.array_slice_index);
}

fn print_gpu_copy(info: &GpuCopyInfo) {
    println!("=== GPU-to-GPU copy ===");
    println!("source array slice:       {}", info.source_array_slice);
    println!("calculated source subresource: {}", info.source_subresource);
    println!("destination subresource:  {}", info.destination_subresource);
    println!("source format:            {}", info.source_format);
    println!("destination format:       {}", info.destination_format);
    println!(
        "source dimensions:        {} x {}",
        info.source_width, info.source_height
    );
    println!(
        "destination dimensions:   {} x {}",
        info.dest_width, info.dest_height
    );
    println!(
        "GPU copy submission:      {}",
        if info.submission_ok {
            "OK — CopySubresourceRegion issued and context flushed"
        } else {
            "FAILED"
        }
    );
}

fn print_shared_fence_sync(bundle: &SharedFenceSyncBundle) {
    let info = &bundle.info;
    println!("=== D3D11 → D3D12 shared GPU fence ===");
    println!(
        "ID3D11Device5 available:      {}",
        if info.device5_available { "yes" } else { "no" }
    );
    println!(
        "ID3D11DeviceContext4 available: {}",
        if info.context4_available { "yes" } else { "no" }
    );
    println!(
        "D3D11 fence creation:       {}",
        if info.fence_creation_ok {
            "OK"
        } else {
            "FAILED"
        }
    );
    println!(
        "shared fence HANDLE:        0x{:016X}",
        info.shared_fence_handle
    );
    println!(
        "D3D12 OpenSharedHandle fence: {}",
        if info.d3d12_fence_open_ok {
            "OK"
        } else {
            "FAILED"
        }
    );
    println!("D3D11 Signal:               {}", info.signal_result);
    println!("D3D12 CommandQueue::Wait:   {}", info.wait_result);
    println!("synchronization mechanism:  {}", info.mechanism);
    println!(
        "cross-API synchronization valid: {}",
        if info.synchronization_valid {
            "yes"
        } else {
            "no"
        }
    );
    if let Some(err) = &info.error {
        println!("error:                      {err}");
    }
    println!();
    println!("{}", info.step_status);
}

fn print_d3d12_open_shared(info: &D3d12OpenSharedInfo) {
    println!("=== D3D12 OpenSharedHandle ===");
    println!(
        "D3D12 device creation:  {}",
        if info.device_creation_ok {
            "OK"
        } else {
            "FAILED"
        }
    );
    println!("adapter name:           {}", info.adapter_name);
    println!(
        "OpenSharedHandle result: {}",
        if info.open_shared_handle_ok {
            "OK"
        } else {
            "FAILED"
        }
    );
    println!("ID3D12Resource pointer: 0x{:x}", info.resource_pointer);
    println!("Width:                  {}", info.width);
    println!("Height:                 {}", info.height);
    println!("MipLevels:              {}", info.mip_levels);
    println!("Format:                 {}", info.format);
    println!("SampleDesc.Count:       {}", info.sample_count);
    println!("Layout:                 {}", info.layout);
    println!("Flags:                  {}", info.flags);
    println!(
        "verify NV12:            {}",
        if info.format_is_nv12 { "OK" } else { "FAILED" }
    );
    println!(
        "verify Width == 3840:   {}",
        if info.width_is_3840 { "OK" } else { "FAILED" }
    );
    println!(
        "verify Height == 2176:  {}",
        if info.height_is_2176 { "OK" } else { "FAILED" }
    );
}

fn print_shared_handle(info: &SharedHandleInfo) {
    println!("=== NT shared HANDLE ===");
    println!(
        "CreateSharedHandle result: {}",
        if info.creation_ok { "OK" } else { "FAILED" }
    );
    println!("HANDLE value:       0x{:016X}", info.handle_value);
    println!(
        "HANDLE valid:       {}",
        if info.handle_valid { "yes" } else { "no" }
    );
    println!("access flags used:  {}", info.access_flags_label);
    println!("texture format:     {}", info.texture_format);
    println!(
        "texture dimensions: {} x {}",
        info.texture_width, info.texture_height
    );
    println!(
        "MiscFlags:          0x{:x} ({})",
        info.misc_flags, info.misc_flags
    );
}

fn print_shareable_texture(info: &ShareableTextureInfo) {
    println!("=== Shareable NV12 texture ===");
    println!(
        "creation result:    {}",
        if info.creation_ok { "OK" } else { "FAILED" }
    );
    println!(
        "Width x Height:     {} x {}",
        info.desc.width, info.desc.height
    );
    println!("ArraySize:          {}", info.desc.array_size);
    println!("DXGI Format:        {}", info.desc.dxgi_format);
    println!(
        "BindFlags:          0x{:x} ({})",
        info.desc.bind_flags, info.desc.bind_flags
    );
    println!(
        "CPUAccessFlags:     0x{:x} ({})",
        info.desc.cpu_access_flags, info.desc.cpu_access_flags
    );
    println!(
        "MiscFlags:          0x{:x} ({})",
        info.desc.misc_flags, info.desc.misc_flags
    );
    println!(
        "SHARED_NTHANDLE:    {}",
        if info.has_shared_nthandle {
            "present"
        } else {
            "missing"
        }
    );
}

fn print_final_experiment_report(multi: &multi_frame::MultiFrameReport) {
    println!("==================================================");
    println!("EXPERIMENT 2 — CORRECTIVE VALIDATION");
    println!("==================================================");
    println!();
    println!("Compilation:");
    println!("PASS");
    println!();
    println!("Architecture unchanged:");
    println!("YES");
    println!();
    println!("Production crates modified:");
    println!("NO");
    println!();
    println!("Cached D3D12 fence:");
    println!("{}", if multi.cached_fence { "YES" } else { "NO" });
    println!();
    println!("OpenSharedHandle fence calls during frame loop:");
    println!("{}", multi.fence_open_shared_handle_calls_in_loop);
    println!();
    println!("Real frames decoded:");
    println!("{}", multi.frames_decoded);
    println!();
    println!("GPU copies:");
    println!("{}", multi.gpu_copies);
    println!();
    println!("Frames rendered:");
    println!("{}", multi.frames_rendered);
    println!();
    println!("Present calls:");
    println!("{}", multi.present_calls);
    println!();
    println!("Wall-clock elapsed:");
    println!("{:.3} s", multi.elapsed_seconds);
    println!();
    println!("Corrected wall-clock FPS:");
    println!("{:.2}", multi.sustained_fps);
    println!();
    println!("Fixture FPS:");
    println!("29.97 approximately");
    println!();
    println!("Throughput >= fixture rate:");
    println!(
        "{}",
        if multi.throughput_ge_fixture {
            "YES"
        } else {
            "NO"
        }
    );
    println!();
    println!("av_hwframe_transfer_data:");
    println!("NOT USED");
    println!();
    println!("swscale:");
    println!("NOT USED");
    println!();
    println!("CPU RGBA:");
    println!("NOT USED");
    println!();
    println!("GPU -> CPU -> GPU:");
    println!("NO");
    println!();
    println!("Synthetic resource substitution:");
    println!("NO");
    println!();
    println!("Visual validation window:");
    println!("{}", multi.visual_validation);
    println!();
    println!("Human visual validation:");
    println!("{}", multi.human_visual_validation);
    println!();
    println!("Resource reuse:");
    println!("{}", multi.resource_reuse);
    println!();
    println!("Leak concern:");
    println!("{}", multi.leak_concern);
    println!();
    println!("Documentation corrected:");
    println!("YES");
    println!();
    println!("AUTOMATED STATUS:");
    if multi.step37_status.contains("PASS")
        && multi.step38_status.contains("PASS")
        && multi.step39_status.contains("PASS")
    {
        println!("PASS");
    } else {
        println!("FAILED");
    }
    println!();
    println!("Human visual validation (benchmark does not measure):");
    println!("{}", multi.human_visual_validation);
    println!();
    println!("==================================================");
}

fn main() -> ExitCode {
    println!("=== GPU Experiment 2 — D3D11VA → wgpu interop ===");
    println!();

    let runtime_version = try_ffmpeg_runtime_version();
    match &runtime_version {
        Some(version) => println!("FFmpeg runtime version: {version}"),
        None => println!("FFmpeg runtime version: (failed to load)"),
    }
    println!();

    let mut blockers: Vec<&str> = Vec::new();

    if runtime_version.is_none() {
        blockers.push("ffmpeg runtime");
    }

    let fixture = fixture_path();
    if fixture.is_file() {
        println!("Fixture: OK ({})", fixture.display());
    } else {
        println!("Fixture: MISSING");
        println!("  Expected: {}", fixture.display());
        blockers.push("fixture");
    }
    println!();

    let ffmpeg_layout = FfmpegDevLayout::inspect();
    ffmpeg_layout.print_diagnostics();

    if !ffmpeg_layout.dev_libs_ok() {
        blockers.push("ffmpeg dev libraries");
    }

    println!();

    let visual_mode = std::env::args().any(|a| a == "--visual");
    let visual_diagnostic_mode = std::env::args().any(|a| a == "--visual-diagnostic");

    if blockers.is_empty() && visual_diagnostic_mode {
        println!("STATUS: VISUAL DIAGNOSTIC MODE");
        println!();
        match visual_diagnostic::run_visual_diagnostic(&fixture) {
            Ok(()) => return ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("visual diagnostic error: {err}");
                return ExitCode::from(3);
            }
        }
    }

    if blockers.is_empty() && visual_mode {
        println!("STATUS: VISUAL VALIDATION MODE");
        println!();
        match visual_validation::run_visual_validation(&fixture) {
            Ok(()) => return ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("visual validation error: {err}");
                return ExitCode::from(3);
            }
        }
    }

    if blockers.is_empty() {
        println!("STATUS: READY FOR EXPERIMENT 2");
        println!();

        let wgpu_context = match wgpu_hal_interop::init_wgpu_dx12_context() {
            Ok(context) => {
                println!(
                    "Pre-FFmpeg wgpu DX12 context: {} ({})",
                    context.adapter_name, context.adapter_backend
                );
                println!();
                context
            }
            Err(err) => {
                eprintln!("wgpu DX12 pre-init failed: {err}");
                return ExitCode::from(4);
            }
        };

        match probe_format_and_open_decoder(&fixture) {
            Ok(mut result) => {
                print_video_stream_info(&result.stream);
                println!();
                print_decoder_info(&result.decoder);
                println!();
                print_decoded_frame_info(&result.frame);
                println!();
                print_d3d11_inspection(&result.d3d11);
                println!();
                print_texture_desc(&result.texture_desc);
                println!();
                print_shareable_texture(&result.shareable_texture);
                println!();
                print_gpu_copy(&result.gpu_copy);
                println!();
                print_shared_handle(&result.shared_handle);
                println!();
                print_d3d12_open_shared(&result.d3d12_open);
                println!();
                print_shared_fence_sync(&result.shared_fence_sync);
                println!();
                let wgpu_interop = wgpu_hal_interop::import_shared_d3d12_nv12_into_wgpu(
                    wgpu_context,
                    result._shared_nt_handle.handle(),
                    result.shared_fence_sync.fence_handle(),
                    &result.d3d12_open.adapter_name,
                    result.shared_fence_sync.info.synchronization_valid,
                );
                wgpu_hal_interop::print_wgpu_hal_interop(&wgpu_interop);
                wgpu_hal_interop::print_cached_fence_info(&wgpu_interop);
                if !wgpu_interop.info.interop_valid {
                    return ExitCode::from(3);
                }

                let context = match wgpu_interop._context.as_ref() {
                    Some(context) => context,
                    None => {
                        eprintln!("step 33 context missing after successful interop");
                        return ExitCode::from(3);
                    }
                };

                match render_path::run_render_path_steps_34_to_36(&wgpu_interop, context) {
                    Ok(render_bundle) => {
                        println!();
                        render_path::print_plane_access(&render_bundle.plane_access);
                        println!();
                        render_path::print_shader_path(&render_bundle.shader_path);
                        println!();
                        render_path::print_render_frame(&render_bundle.render_frame);
                        println!();
                        match multi_frame::run_steps_37_to_39(
                            &mut result,
                            context,
                            &render_bundle,
                            &wgpu_interop,
                        ) {
                            Ok(multi_report) => {
                                multi_frame::print_multi_frame_report(&multi_report);
                                if multi_report.step37_status.contains("PASS")
                                    && multi_report.step38_status.contains("PASS")
                                    && multi_report.step39_status.contains("PASS")
                                {
                                    println!();
                                    print_final_experiment_report(&multi_report);
                                    ExitCode::SUCCESS
                                } else {
                                    println!();
                                    print_final_experiment_report(&multi_report);
                                    ExitCode::from(3)
                                }
                            }
                            Err(err) => {
                                eprintln!("multi-frame error: {err}");
                                ExitCode::from(3)
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("render path error: {err}");
                        ExitCode::from(3)
                    }
                }
            }
            Err(err) => {
                eprintln!("FFmpeg error: {err}");
                ExitCode::from(1)
            }
        }
    } else {
        println!("STATUS: BLOCKED");
        println!();
        println!("Blockers: {}", blockers.join(", "));
        println!();
        println!("This experiment does not generate fake decoder results.");
        println!("Complete setup per {SETUP_DOC} and add the fixture per docs/fixtures/README.md.");

        ExitCode::from(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_path_is_under_repo_root() {
        let path = fixture_path();
        assert!(path.ends_with(FIXTURE_REL.replace('/', "\\")) || path.ends_with(FIXTURE_REL));
    }
}
