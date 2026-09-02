//! Explicit wake/work state machine for the winit event loop.
//!
//! Expired playback deadlines are never discarded: they become [`Self::playback_due`].
//! Only strictly future instants are passed to `ControlFlow::WaitUntil`.

#![allow(clippy::collapsible_if)]

use std::time::{Duration, Instant};

/// Selected blocking mode for the winit event loop.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ControlFlowAction {
    /// Sleep until the provided future instant.
    WaitUntil(Instant),
    /// Sleep until the next external event.
    Wait,
}

/// Explicit schedule separating playback, UI repaint, and surface/resize redraw work.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct EventLoopSchedule {
    playback_deadline: Option<Instant>,
    egui_deadline: Option<Instant>,

    playback_due: bool,
    ui_redraw_due: bool,
    surface_redraw_due: bool,

    redraw_requested: bool,
    outstanding_egui_redraw_request: bool,
}

impl EventLoopSchedule {
    /// Clears playback-specific deadlines and due flags.
    pub fn clear_playback(&mut self) {
        self.playback_deadline = None;
        self.playback_due = false;
    }

    /// Updates the playback wakeup instant and due flag from pipeline state.
    pub fn sync_playback(&mut self, deadline: Option<Instant>, frame_due: bool, now: Instant) {
        self.playback_deadline = deadline;
        if frame_due {
            self.playback_due = true;
        } else if let Some(instant) = deadline {
            if instant <= now {
                self.playback_due = true;
            }
        }
    }

    /// Records an egui repaint delay. Zero delay becomes one bounded UI redraw due-now.
    pub fn note_egui_repaint_delay(&mut self, delay: Duration, now: Instant) {
        if delay.is_zero() {
            self.ui_redraw_due = true;
            self.egui_deadline = None;
        } else {
            self.egui_deadline = now.checked_add(delay);
        }
    }

    /// Clears a previously recorded non-zero egui repaint delay.
    pub const fn clear_egui_deadline(&mut self) {
        self.egui_deadline = None;
    }

    pub const fn mark_surface_redraw_due(&mut self) {
        self.surface_redraw_due = true;
    }

    pub const fn clear_surface_redraw_due(&mut self) {
        self.surface_redraw_due = false;
    }

    /// Requests a platform redraw. Returns `false` when an identical egui request is coalesced.
    pub fn request_redraw(&mut self, coalesce_egui: bool) -> bool {
        if coalesce_egui {
            if self.outstanding_egui_redraw_request {
                return false;
            }
            self.outstanding_egui_redraw_request = true;
        }
        self.redraw_requested = true;
        true
    }

    /// Called when `WindowEvent::RedrawRequested` is received.
    pub const fn on_redraw_requested_event(&mut self) {
        self.outstanding_egui_redraw_request = false;
    }

    pub const fn clear_redraw_request(&mut self) {
        self.redraw_requested = false;
    }

    pub const fn clear_ui_redraw_due(&mut self) {
        self.ui_redraw_due = false;
    }

    /// Promotes expired instants into due flags without discarding the represented work.
    pub fn refresh(&mut self, now: Instant) {
        if let Some(deadline) = self.playback_deadline {
            if deadline <= now {
                self.playback_due = true;
            }
        }
        if let Some(deadline) = self.egui_deadline {
            if deadline <= now {
                self.ui_redraw_due = true;
                self.egui_deadline = None;
            }
        }
    }

    #[allow(dead_code)]
    pub const fn playback_due(&self) -> bool {
        self.playback_due
    }

    pub fn consume_playback_due(&mut self) -> bool {
        let due = self.playback_due;
        self.playback_due = false;
        due
    }

    pub const fn surface_redraw_due(&self) -> bool {
        self.surface_redraw_due
    }

    pub const fn ui_redraw_due(&self) -> bool {
        self.ui_redraw_due
    }

    pub const fn redraw_requested(&self) -> bool {
        self.redraw_requested
    }

    /// Returns whether any immediate work should run before sleeping.
    pub const fn has_immediate_work(&self, playing: bool) -> bool {
        if playing && self.playback_due {
            return true;
        }
        if self.surface_redraw_due {
            return true;
        }
        if self.ui_redraw_due || self.redraw_requested {
            return true;
        }
        false
    }

    /// Selects the next sleep target. Never returns an expired instant.
    pub fn control_flow(&self, now: Instant) -> ControlFlowAction {
        let future_playback = self.playback_deadline.filter(|deadline| *deadline > now);
        let future_egui = self.egui_deadline.filter(|deadline| *deadline > now);

        match (future_playback, future_egui) {
            (Some(a), Some(b)) => ControlFlowAction::WaitUntil(a.min(b)),
            (Some(a), None) | (None, Some(a)) => ControlFlowAction::WaitUntil(a),
            (None, None) => ControlFlowAction::Wait,
        }
    }

    pub(crate) fn diagnostic_fields(
        &self,
        now: Instant,
    ) -> (Option<i64>, Option<i64>, bool, bool, bool, bool, bool) {
        (
            Self::deadline_relative_ms(self.playback_deadline, now),
            Self::deadline_relative_ms(self.egui_deadline, now),
            self.playback_due,
            self.ui_redraw_due,
            self.surface_redraw_due,
            self.redraw_requested,
            self.outstanding_egui_redraw_request,
        )
    }

    fn deadline_relative_ms(deadline: Option<Instant>, now: Instant) -> Option<i64> {
        deadline.map(|instant| {
            if instant > now {
                instant.duration_since(now).as_millis() as i64
            } else {
                -(now.duration_since(instant).as_millis() as i64)
            }
        })
    }
}

/// Returns whether an egui repaint delay should schedule a one-shot redraw request.
pub const fn egui_delay_requests_immediate_redraw(delay: Duration) -> bool {
    delay.is_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn future_playback_deadline_waits_until() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(30);
        let schedule = EventLoopSchedule {
            playback_deadline: Some(deadline),
            ..Default::default()
        };
        assert_eq!(
            schedule.control_flow(now),
            ControlFlowAction::WaitUntil(deadline)
        );
        assert!(!schedule.playback_due());
    }

    #[test]
    fn expired_playback_deadline_becomes_due_now_not_wait_until() {
        let now = Instant::now();
        let expired = now - Duration::from_millis(1);
        let mut schedule = EventLoopSchedule {
            playback_deadline: Some(expired),
            ..Default::default()
        };
        schedule.refresh(now);
        assert!(schedule.playback_due());
        assert_eq!(schedule.control_flow(now), ControlFlowAction::Wait);
        assert!(schedule.has_immediate_work(true));
    }

    #[test]
    fn sync_playback_marks_due_when_pipeline_reports_frame_due() {
        let now = Instant::now();
        let mut schedule = EventLoopSchedule::default();
        schedule.sync_playback(None, true, now);
        assert!(schedule.playback_due());
    }

    #[test]
    fn expired_egui_deadline_becomes_one_bounded_ui_redraw() {
        let now = Instant::now();
        let mut schedule = EventLoopSchedule {
            egui_deadline: Some(now - Duration::from_millis(1)),
            ..Default::default()
        };
        schedule.refresh(now);
        assert!(schedule.ui_redraw_due());
        assert_eq!(schedule.egui_deadline, None);
        assert_eq!(schedule.control_flow(now), ControlFlowAction::Wait);
    }

    #[test]
    fn zero_egui_delay_sets_ui_redraw_due_now() {
        let now = Instant::now();
        let mut schedule = EventLoopSchedule::default();
        schedule.note_egui_repaint_delay(Duration::ZERO, now);
        assert!(schedule.ui_redraw_due());
        assert_eq!(schedule.egui_deadline, None);
    }

    #[test]
    fn future_egui_deadline_earlier_than_playback() {
        let now = Instant::now();
        let schedule = EventLoopSchedule {
            playback_deadline: Some(now + Duration::from_millis(30)),
            egui_deadline: Some(now + Duration::from_millis(10)),
            ..Default::default()
        };
        assert_eq!(
            schedule.control_flow(now),
            ControlFlowAction::WaitUntil(now + Duration::from_millis(10))
        );
    }

    #[test]
    fn coalesced_egui_redraw_requests() {
        let mut schedule = EventLoopSchedule::default();
        assert!(schedule.request_redraw(true));
        assert!(!schedule.request_redraw(true));
        schedule.on_redraw_requested_event();
        assert!(schedule.request_redraw(true));
    }

    #[test]
    fn resize_redraw_is_not_coalesced() {
        let mut schedule = EventLoopSchedule::default();
        assert!(schedule.request_redraw(false));
        assert!(schedule.request_redraw(false));
    }

    #[test]
    fn immediate_work_includes_surface_redraw() {
        let schedule = EventLoopSchedule {
            surface_redraw_due: true,
            ..Default::default()
        };
        assert!(schedule.has_immediate_work(false));
    }
}
