//! Interactive resize diagnostic logging (`--diagnose-resize`).
//!
//! Writes `target/dvs-resize-diagnostic.log` with bounded, collapsible entries.
//! Inactive unless explicitly enabled via CLI.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use dvs_playback::PlaybackState;
use winit::event::StartCause;

use crate::event_loop_schedule::ControlFlowAction;
use crate::state::AppState;

const LOG_PATH: &str = "target/dvs-resize-diagnostic.log";
const MAX_ENTRIES: usize = 4_096;

/// Whether an event kind is preserved when the entry cap is reached.
fn is_critical_kind(kind: &str) -> bool {
    matches!(
        kind,
        "state_transition"
            | "playback_started"
            | "eof_reached"
            | "surface_acquire_error"
            | "scheduler_fatal"
            | "deadline_promoted"
            | "stall_snapshot_f8"
            | "initialization_complete"
            | "diagnostic_startup"
            | "diagnostic_cap_reached"
    )
}

/// Snapshot of scheduling + pipeline state for one log line.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticSnapshot {
    pub kind: String,
    pub start_cause: Option<String>,
    pub app_state: AppState,
    pub window_width: u32,
    pub window_height: u32,
    pub surface_width: u32,
    pub surface_height: u32,
    pub scale_factor: f64,
    pub surface_configured: bool,
    pub redraw_outstanding: bool,
    pub schedule_playback_deadline_ms: Option<i64>,
    pub schedule_egui_deadline_ms: Option<i64>,
    pub schedule_playback_due: bool,
    pub schedule_ui_redraw_due: bool,
    pub schedule_surface_redraw_due: bool,
    pub schedule_redraw_requested: bool,
    pub schedule_egui_coalesce_outstanding: bool,
    pub prepared_present: bool,
    pub prepared_frame_id: Option<u64>,
    pub prepared_pts: Option<i64>,
    pub bridge_state: String,
    pub playback_clock_state: PlaybackState,
    pub media_position_us: Option<i64>,
    pub frames_decoded: u64,
    pub frames_presented: u64,
    pub display_only_redraw_count: u64,
    pub frames_dropped_late: u64,
    pub frames_rejected: u64,
    pub eof: bool,
    pub selected_control_flow: String,
    pub last_scheduler_decision: String,
    pub last_surface_acquire: String,
    pub last_submit_ms: Option<u64>,
    pub last_present_ms: Option<u64>,
    pub process_work_depth: u32,
    pub extra: String,
}

impl DiagnosticSnapshot {
    fn collapse_key(&self) -> String {
        format!(
            "{}|{}|{:?}|{}x{}|{}x{}|{:.3}|{}|{}|pb{:?}|eg{:?}|pd{}|ui{}|sr{}|rr{}|eo{}|pp{}|pf{:?}|pt{:?}|{}|{:?}|mp{:?}|d{}|p{}|dr{}|dl{}|rj{}|eof{}|cf{}|sd{}|sa{}|ls{:?}|lp{:?}|d{}|{}",
            self.kind,
            self.start_cause.as_deref().unwrap_or("-"),
            self.app_state,
            self.window_width,
            self.window_height,
            self.surface_width,
            self.surface_height,
            self.scale_factor,
            self.surface_configured,
            self.redraw_outstanding,
            self.schedule_playback_deadline_ms,
            self.schedule_egui_deadline_ms,
            self.schedule_playback_due,
            self.schedule_ui_redraw_due,
            self.schedule_surface_redraw_due,
            self.schedule_redraw_requested,
            self.schedule_egui_coalesce_outstanding,
            self.prepared_present,
            self.prepared_frame_id,
            self.prepared_pts,
            self.bridge_state,
            self.playback_clock_state,
            self.media_position_us,
            self.frames_decoded,
            self.frames_presented,
            self.display_only_redraw_count,
            self.frames_dropped_late,
            self.frames_rejected,
            self.eof,
            self.selected_control_flow,
            self.last_scheduler_decision,
            self.last_surface_acquire,
            self.last_submit_ms,
            self.last_present_ms,
            self.process_work_depth,
            self.extra,
        )
    }

    fn format_line(&self, seq: u64, elapsed_ms: u64) -> String {
        format!(
            "#{seq:05} t={elapsed_ms}ms kind={} start={} app={:?} win={}x{} surf={}x{} scale={:.3} surf_ok={} redraw_out={} \
pb_deadline_ms={} eg_deadline_ms={} playback_due={} ui_due={} surface_due={} redraw_req={} egui_coalesce={} \
prepared={} frame_id={} pts={} bridge={} clock={:?} media_us={} decoded={} presented={} display_only={} \
late_drop={} rejected={} eof={} control_flow={} scheduler={} acquire={} last_submit_ms={} last_present_ms={} \
work_depth={} extra={}",
            self.kind,
            self.start_cause.as_deref().unwrap_or("-"),
            self.app_state,
            self.window_width,
            self.window_height,
            self.surface_width,
            self.surface_height,
            self.scale_factor,
            self.surface_configured,
            self.redraw_outstanding,
            opt_i64(self.schedule_playback_deadline_ms),
            opt_i64(self.schedule_egui_deadline_ms),
            self.schedule_playback_due,
            self.schedule_ui_redraw_due,
            self.schedule_surface_redraw_due,
            self.schedule_redraw_requested,
            self.schedule_egui_coalesce_outstanding,
            self.prepared_present,
            opt_u64(self.prepared_frame_id),
            opt_i64(self.prepared_pts),
            self.bridge_state,
            self.playback_clock_state,
            opt_i64(self.media_position_us),
            self.frames_decoded,
            self.frames_presented,
            self.display_only_redraw_count,
            self.frames_dropped_late,
            self.frames_rejected,
            self.eof,
            self.selected_control_flow,
            self.last_scheduler_decision,
            self.last_surface_acquire,
            opt_u64(self.last_submit_ms),
            opt_u64(self.last_present_ms),
            self.process_work_depth,
            self.extra,
        )
    }
}

fn opt_i64(value: Option<i64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn opt_u64(value: Option<u64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string())
}

pub fn log_path() -> PathBuf {
    PathBuf::from(LOG_PATH)
}

/// Interactive resize diagnostic logger.
pub struct ResizeDiagnostic {
    writer: BufWriter<File>,
    started: Instant,
    seq: u64,
    entries: usize,
    cap_reached: bool,
    last_key: Option<String>,
    repeat_count: u64,
    display_only_redraw_count: u64,
    last_scheduler_decision: String,
    last_surface_acquire: String,
    last_submit_ms: Option<u64>,
    last_present_ms: Option<u64>,
    process_work_depth: u32,
}

impl ResizeDiagnostic {
    pub fn open() -> Result<Self, std::io::Error> {
        let path = log_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        let mut diag = Self {
            writer: BufWriter::new(file),
            started: Instant::now(),
            seq: 0,
            entries: 0,
            cap_reached: false,
            last_key: None,
            repeat_count: 0,
            display_only_redraw_count: 0,
            last_scheduler_decision: "-".to_string(),
            last_surface_acquire: "-".to_string(),
            last_submit_ms: None,
            last_present_ms: None,
            process_work_depth: 0,
        };
        diag.write_raw("=== dvs-app resize diagnostic startup ===")?;
        diag.record_kind("diagnostic_startup", "-")?;
        Ok(diag)
    }

    pub const fn display_only_redraw_count(&self) -> u64 {
        self.display_only_redraw_count
    }

    pub fn record_display_only_redraw(&mut self) {
        self.display_only_redraw_count = self.display_only_redraw_count.saturating_add(1);
    }

    pub fn set_scheduler_decision(&mut self, decision: impl Into<String>) {
        self.last_scheduler_decision = decision.into();
    }

    pub fn set_surface_acquire(&mut self, result: impl Into<String>) {
        self.last_surface_acquire = result.into();
    }

    pub fn record_queue_submit(&mut self) {
        self.last_submit_ms = Some(self.elapsed_ms());
    }

    pub fn record_surface_present(&mut self) {
        self.last_present_ms = Some(self.elapsed_ms());
    }

    pub(crate) fn last_scheduler_decision(&self) -> &str {
        &self.last_scheduler_decision
    }

    pub(crate) fn last_surface_acquire(&self) -> &str {
        &self.last_surface_acquire
    }

    pub(crate) const fn last_submit_ms(&self) -> Option<u64> {
        self.last_submit_ms
    }

    pub(crate) const fn last_present_ms(&self) -> Option<u64> {
        self.last_present_ms
    }

    pub(crate) const fn process_work_depth(&self) -> u32 {
        self.process_work_depth
    }

    pub fn process_work_enter(&mut self) {
        self.process_work_depth = self.process_work_depth.saturating_add(1);
    }

    pub fn process_work_exit(&mut self) {
        self.process_work_depth = self.process_work_depth.saturating_sub(1);
    }

    pub fn record_redraw_coalesced(&mut self) {
        let _ = self.record_kind("redraw_coalesced", "egui");
    }

    pub fn record_deadline_promoted(&mut self, which: &str) {
        let _ = self.record_kind("deadline_promoted", which);
    }

    pub fn log_snapshot(&mut self, snapshot: DiagnosticSnapshot) -> Result<(), std::io::Error> {
        if self.cap_reached && !is_critical_kind(&snapshot.kind) {
            return Ok(());
        }
        let key = snapshot.collapse_key();
        if self.last_key.as_deref() == Some(key.as_str()) {
            self.repeat_count = self.repeat_count.saturating_add(1);
            return Ok(());
        }
        self.flush_repeat()?;
        self.last_key = Some(key);
        self.write_entry(&snapshot)
    }

    pub fn record_kind(&mut self, kind: &str, extra: &str) -> Result<(), std::io::Error> {
        self.log_snapshot(DiagnosticSnapshot {
            kind: kind.to_string(),
            extra: extra.to_string(),
            ..DiagnosticSnapshot::default()
        })
    }

    pub fn write_stall_snapshot_f8(
        &mut self,
        snapshot: DiagnosticSnapshot,
    ) -> Result<(), std::io::Error> {
        self.flush_repeat()?;
        let mut snap = snapshot;
        snap.kind = "stall_snapshot_f8".to_string();
        snap.extra = "F8 manual snapshot".to_string();
        self.write_entry(&snap)
    }

    pub fn finish(&mut self) -> Result<(), std::io::Error> {
        self.flush_repeat()?;
        self.write_raw("=== dvs-app resize diagnostic shutdown ===")?;
        self.writer.flush()
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn flush_repeat(&mut self) -> Result<(), std::io::Error> {
        if self.repeat_count > 1 {
            self.write_raw(&format!("REPEATED x{}", self.repeat_count))?;
            self.repeat_count = 0;
        } else {
            self.repeat_count = 0;
        }
        Ok(())
    }

    fn write_entry(&mut self, snapshot: &DiagnosticSnapshot) -> Result<(), std::io::Error> {
        if self.entries >= MAX_ENTRIES {
            if !self.cap_reached {
                self.cap_reached = true;
                self.write_raw(
                    "=== diagnostic entry cap reached; preserving critical entries only ===",
                )?;
                self.write_raw("diagnostic_cap_reached")?;
            }
            if !is_critical_kind(&snapshot.kind) {
                return Ok(());
            }
        }
        self.seq = self.seq.saturating_add(1);
        self.entries = self.entries.saturating_add(1);
        let line = snapshot.format_line(self.seq, self.elapsed_ms());
        self.write_raw(&line)
    }

    fn write_raw(&mut self, line: &str) -> Result<(), std::io::Error> {
        writeln!(self.writer, "{line}")?;
        self.writer.flush()
    }
}

impl Default for DiagnosticSnapshot {
    fn default() -> Self {
        Self {
            kind: String::new(),
            start_cause: None,
            app_state: AppState::Initializing,
            window_width: 0,
            window_height: 0,
            surface_width: 0,
            surface_height: 0,
            scale_factor: 1.0,
            surface_configured: false,
            redraw_outstanding: false,
            schedule_playback_deadline_ms: None,
            schedule_egui_deadline_ms: None,
            schedule_playback_due: false,
            schedule_ui_redraw_due: false,
            schedule_surface_redraw_due: false,
            schedule_redraw_requested: false,
            schedule_egui_coalesce_outstanding: false,
            prepared_present: false,
            prepared_frame_id: None,
            prepared_pts: None,
            bridge_state: "-".to_string(),
            playback_clock_state: PlaybackState::Stopped,
            media_position_us: None,
            frames_decoded: 0,
            frames_presented: 0,
            display_only_redraw_count: 0,
            frames_dropped_late: 0,
            frames_rejected: 0,
            eof: false,
            selected_control_flow: "-".to_string(),
            last_scheduler_decision: "-".to_string(),
            last_surface_acquire: "-".to_string(),
            last_submit_ms: None,
            last_present_ms: None,
            process_work_depth: 0,
            extra: String::new(),
        }
    }
}

pub fn start_cause_label(cause: &StartCause) -> String {
    match cause {
        StartCause::ResumeTimeReached {
            start,
            requested_resume,
        } => format!(
            "ResumeTimeReached(start+{}ms req+{}ms)",
            start.elapsed().as_millis(),
            requested_resume.elapsed().as_millis()
        ),
        StartCause::WaitCancelled {
            start,
            requested_resume,
        } => format!(
            "WaitCancelled(start+{}ms resume={})",
            start.elapsed().as_millis(),
            requested_resume
                .map(|instant| {
                    instant
                        .checked_duration_since(Instant::now())
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                        .to_string()
                })
                .unwrap_or_else(|| "-".to_string())
        ),
        StartCause::Poll => "Poll".to_string(),
        StartCause::Init => "Init".to_string(),
    }
}

pub fn control_flow_action_label(action: ControlFlowAction) -> String {
    match action {
        ControlFlowAction::Wait => "Wait".to_string(),
        ControlFlowAction::WaitUntil(instant) => {
            let ms = instant
                .checked_duration_since(Instant::now())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            format!("WaitUntil(+{ms}ms)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_kinds_are_recognized() {
        assert!(is_critical_kind("stall_snapshot_f8"));
        assert!(!is_critical_kind("about_to_wait"));
    }

    #[test]
    fn collapse_key_changes_when_state_changes() {
        let mut a = DiagnosticSnapshot {
            kind: "test".to_string(),
            app_state: AppState::Playing,
            ..DiagnosticSnapshot::default()
        };
        let mut b = a.clone();
        b.app_state = AppState::Ended;
        assert_ne!(a.collapse_key(), b.collapse_key());
        let _ = &mut a;
    }
}
