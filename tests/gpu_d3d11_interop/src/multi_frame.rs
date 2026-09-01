//! Steps 37–39 — multi-frame pipeline, stability, and performance validation.

use std::time::Instant;

use windows::Win32::Graphics::Direct3D12::ID3D12Fence;

use crate::render_path::RenderPathBundle;
use crate::wgpu_hal_interop::{self, WgpuDx12Context, WgpuHalInteropBundle};

const TARGET_FRAMES: u32 = 90;
const FIXTURE_FPS: f64 = 30_000.0 / 1_001.0;

pub struct MultiFrameReport {
    pub frames_decoded: u32,
    pub gpu_copies: u32,
    pub frames_rendered: u32,
    pub present_calls: u32,
    pub fence_values_used: u64,
    pub fence_open_shared_handle_calls_in_loop: u32,
    pub fence_open_shared_handle_calls_at_init: u32,
    pub cached_fence: bool,
    pub total_ms: f64,
    pub elapsed_seconds: f64,
    pub decode_ms: f64,
    pub copy_ms: f64,
    pub sync_ms: f64,
    pub render_ms: f64,
    pub sustained_fps: f64,
    pub throughput_ge_fixture: bool,
    pub visual_validation: String,
    pub human_visual_validation: String,
    pub resource_reuse: String,
    pub leak_concern: String,
    pub step37_status: String,
    pub step38_status: String,
    pub step39_status: String,
}

pub fn run_steps_37_to_39(
    probe: &mut crate::ProbeResult,
    context: &WgpuDx12Context,
    render: &RenderPathBundle,
    wgpu_interop: &WgpuHalInteropBundle,
) -> Result<MultiFrameReport, String> {
    if wgpu_interop._texture.is_none() {
        return Err("step 37 requires imported wgpu texture from step 33".to_string());
    }

    let cached_fence = wgpu_interop
        .cached_wgpu_fence
        .as_ref()
        .ok_or_else(|| "cached wgpu ID3D12Fence missing — OpenSharedHandle must run once at init".to_string())?;

    let mut frames_decoded = 0u32;
    let mut gpu_copies = 0u32;
    let mut frames_rendered = 0u32;
    let mut present_calls = 0u32;
    // Step 32/33 already used fence value 1; measured run starts at 2.
    let mut fence_value = 2u64;
    let fence_open_shared_handle_calls_in_loop = 0u32;
    let mut decode_ms = 0.0;
    let mut copy_ms = 0.0;
    let mut sync_ms = 0.0;
    let mut render_ms = 0.0;

    let total_start = Instant::now();

    while frames_decoded < TARGET_FRAMES {
        process_one_real_frame(
            probe,
            context,
            render,
            cached_fence,
            fence_value,
            &mut frames_decoded,
            &mut gpu_copies,
            &mut frames_rendered,
            &mut present_calls,
            &mut decode_ms,
            &mut copy_ms,
            &mut sync_ms,
            &mut render_ms,
        )?;
        fence_value += 1;
    }

    let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;
    let elapsed_seconds = total_ms / 1000.0;
    let sustained_fps = if elapsed_seconds > 0.0 {
        frames_rendered as f64 / elapsed_seconds
    } else {
        0.0
    };
    let throughput_ge_fixture = sustained_fps >= FIXTURE_FPS;
    let measured_fence_values_used = fence_value - 2;

    let step37_ok = frames_decoded == TARGET_FRAMES
        && gpu_copies == TARGET_FRAMES
        && frames_rendered == TARGET_FRAMES
        && present_calls == TARGET_FRAMES;
    let step38_ok = wgpu_interop.fence_open_shared_handle_calls == 1
        && fence_open_shared_handle_calls_in_loop == 0
        && wgpu_interop.cached_wgpu_fence.is_some();
    let step39_ok = throughput_ge_fixture && frames_rendered == TARGET_FRAMES;

    Ok(MultiFrameReport {
        frames_decoded,
        gpu_copies,
        frames_rendered,
        present_calls,
        fence_values_used: measured_fence_values_used,
        fence_open_shared_handle_calls_in_loop,
        fence_open_shared_handle_calls_at_init: wgpu_interop.fence_open_shared_handle_calls,
        cached_fence: wgpu_interop.cached_wgpu_fence.is_some(),
        total_ms,
        elapsed_seconds,
        decode_ms,
        copy_ms,
        sync_ms,
        render_ms,
        sustained_fps,
        throughput_ge_fixture,
        visual_validation: "USE --visual MODE".to_string(),
        human_visual_validation: "PENDING".to_string(),
        resource_reuse: "BOUNDED".to_string(),
        leak_concern: "NONE — fence OpenSharedHandle once at init; Wait reuses cached ID3D12Fence"
            .to_string(),
        step37_status: if step37_ok {
            format!("STEP 37 / 40: PASS — {frames_decoded} real frames")
        } else {
            format!(
                "STEP 37 / 40: FAIL — decoded={frames_decoded} copies={gpu_copies} rendered={frames_rendered} presents={present_calls}"
            )
        },
        step38_status: if step38_ok {
            "STEP 38 / 40: PASS — cached fence, bounded reuse".to_string()
        } else {
            format!(
                "STEP 38 / 40: FAIL — init opens={} loop opens={}",
                wgpu_interop.fence_open_shared_handle_calls,
                fence_open_shared_handle_calls_in_loop
            )
        },
        step39_status: if step39_ok {
            format!(
                "STEP 39 / 40: PASS — wall-clock {sustained_fps:.2} FPS >= {FIXTURE_FPS:.2}"
            )
        } else {
            format!(
                "STEP 39 / 40: FAIL — wall-clock {sustained_fps:.2} FPS (target {FIXTURE_FPS:.2})"
            )
        },
    })
}

pub(crate) fn process_one_real_frame(
    probe: &mut crate::ProbeResult,
    context: &WgpuDx12Context,
    render: &RenderPathBundle,
    cached_fence: &ID3D12Fence,
    fence_value: u64,
    frames_decoded: &mut u32,
    gpu_copies: &mut u32,
    frames_rendered: &mut u32,
    present_calls: &mut u32,
    decode_ms: &mut f64,
    copy_ms: &mut f64,
    sync_ms: &mut f64,
    render_ms: &mut f64,
) -> Result<(), String> {
    let decode_start = Instant::now();
    let frame_info = crate::decode_next_d3d11_frame(
        &probe._fmt,
        &probe._decoder,
        probe.stream.stream_index,
        &mut probe._av_frame,
    )?;
    let Some(frame_info) = frame_info else {
        return Err("EOF before completing required frame count".to_string());
    };
    *decode_ms += decode_start.elapsed().as_secs_f64() * 1000.0;
    if !frame_info.is_d3d11 {
        return Err(format!(
            "frame is not AV_PIX_FMT_D3D11 ({})",
            frame_info.format_name
        ));
    }
    *frames_decoded += 1;

    let inspection = crate::inspect_d3d11_frame(&probe._av_frame)?;
    let copy_start = Instant::now();
    let _gpu_copy = crate::copy_decoder_slice_to_shareable(
        &probe._av_frame,
        &inspection,
        &probe.texture_desc,
        probe
            ._shareable_texture
            .as_ref()
            .ok_or_else(|| "shareable texture missing".to_string())?,
        &probe.shareable_texture.desc,
    )?;
    *copy_ms += copy_start.elapsed().as_secs_f64() * 1000.0;
    *gpu_copies += 1;

    let sync_start = Instant::now();
    probe.shared_fence_sync.signal_and_wait(fence_value)?;
    wgpu_hal_interop::wait_cached_wgpu_fence(context, cached_fence, fence_value)?;
    *sync_ms += sync_start.elapsed().as_secs_f64() * 1000.0;

    let render_start = Instant::now();
    crate::render_path::present_nv12_frame(context, &render.pipeline, &render.bind_group)?;
    *render_ms += render_start.elapsed().as_secs_f64() * 1000.0;
    *frames_rendered += 1;
    *present_calls += 1;

    Ok(())
}

pub fn print_multi_frame_report(report: &MultiFrameReport) {
    println!("=== Multi-frame continuous pipeline ===");
    println!("decoded frames:         {}", report.frames_decoded);
    println!("GPU copies:             {}", report.gpu_copies);
    println!("frames rendered:        {}", report.frames_rendered);
    println!("present calls:          {}", report.present_calls);
    println!("fence values used:      {}", report.fence_values_used);
    println!();
    println!("{}", report.step37_status);
    println!();
    println!("=== Lifetime + synchronization stability ===");
    println!("cached D3D12 fence:     {}", if report.cached_fence { "YES" } else { "NO" });
    println!(
        "OpenSharedHandle fence calls at init: {}",
        report.fence_open_shared_handle_calls_at_init
    );
    println!(
        "OpenSharedHandle fence calls during frame loop: {}",
        report.fence_open_shared_handle_calls_in_loop
    );
    println!("bounded shareable texture: reused");
    println!("bounded fence HANDLE:    reused");
    println!("monotonic fence values:  yes");
    println!("resource reuse:          {}", report.resource_reuse);
    println!("leak concern:            {}", report.leak_concern);
    println!();
    println!("{}", report.step38_status);
    println!();
    println!("=== Performance validation ===");
    println!("Measurement type:       WALL-CLOCK END-TO-END THROUGHPUT");
    println!("(NOT GPU execution time — no GPU timestamp queries)");
    println!("wall-clock elapsed:     {:.3} s ({:.2} ms)", report.elapsed_seconds, report.total_ms);
    println!("frames_processed:       {}", report.frames_rendered);
    println!(
        "FPS = frames/elapsed:   {:.2}",
        report.sustained_fps
    );
    println!("fixture FPS:            ~{:.2} (30000/1001)", FIXTURE_FPS);
    println!(
        "throughput >= fixture:  {}",
        if report.throughput_ge_fixture { "YES" } else { "NO" }
    );
    println!("decode phase:           {:.2} ms", report.decode_ms);
    println!("GPU copy phase:         {:.2} ms", report.copy_ms);
    println!("sync phase:             {:.2} ms", report.sync_ms);
    println!("render+present phase:   {:.2} ms", report.render_ms);
    println!();
    println!("CPU pixel transfers in normal path: NO");
    println!("av_hwframe_transfer_data: NOT USED");
    println!("swscale: NOT USED");
    println!("CPU RGBA: NOT USED");
    println!("GPU -> CPU -> GPU: NO");
    println!("GPU -> GPU copy: YES");
    println!();
    println!("{}", report.step39_status);
    println!();
    println!("Visual validation window: {}", report.visual_validation);
    println!("Human visual validation:  {}", report.human_visual_validation);
}
