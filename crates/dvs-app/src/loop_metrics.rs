//! Smoke-test-only event-loop instrumentation.

use std::time::{Duration, Instant};

/// Source of a [`LoopMetrics::request_redraw`] call.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum RedrawRequestSource {
    EguiEventResponse,
    EguiZeroRepaintDelay,
    Resize,
    SurfaceRetry,
    Initialization,
    PostEof,
    SmokePlayingResize,
}

/// Bounded counters for diagnosing event-loop scheduling during smoke playback.
#[derive(Debug, Clone, Default)]
pub struct LoopMetrics {
    pub total_window_events: u64,
    pub about_to_wait_calls: u64,
    pub redraw_requested_events: u64,
    pub request_redraw_calls: u64,
    pub request_redraw_egui_event_response: u64,
    pub request_redraw_egui_zero_delay: u64,
    pub request_redraw_resize: u64,
    pub request_redraw_surface_retry: u64,
    pub request_redraw_initialization: u64,
    pub request_redraw_post_eof: u64,
    pub request_redraw_smoke_playing_resize: u64,
    pub scheduler_evaluations: u64,
    pub scheduler_early_waits: u64,
    pub control_flow_wait_until_future: u64,
    pub control_flow_wait_until_expired: u64,
    pub control_flow_wait: u64,
    pub queue_submissions: u64,
    pub surface_presents: u64,
    pub zero_egui_repaint_delays: u64,
    pub non_zero_egui_repaint_delays: u64,
}

impl LoopMetrics {
    pub fn record_window_event(&mut self) {
        self.total_window_events = self.total_window_events.saturating_add(1);
    }

    pub fn record_about_to_wait(&mut self) {
        self.about_to_wait_calls = self.about_to_wait_calls.saturating_add(1);
    }

    pub fn record_redraw_requested_event(&mut self) {
        self.redraw_requested_events = self.redraw_requested_events.saturating_add(1);
    }

    pub fn record_request_redraw(&mut self, source: RedrawRequestSource) {
        self.request_redraw_calls = self.request_redraw_calls.saturating_add(1);
        match source {
            RedrawRequestSource::EguiEventResponse => {
                self.request_redraw_egui_event_response =
                    self.request_redraw_egui_event_response.saturating_add(1);
            }
            RedrawRequestSource::EguiZeroRepaintDelay => {
                self.request_redraw_egui_zero_delay =
                    self.request_redraw_egui_zero_delay.saturating_add(1);
            }
            RedrawRequestSource::Resize => {
                self.request_redraw_resize = self.request_redraw_resize.saturating_add(1);
            }
            RedrawRequestSource::SurfaceRetry => {
                self.request_redraw_surface_retry =
                    self.request_redraw_surface_retry.saturating_add(1);
            }
            RedrawRequestSource::Initialization => {
                self.request_redraw_initialization =
                    self.request_redraw_initialization.saturating_add(1);
            }
            RedrawRequestSource::PostEof => {
                self.request_redraw_post_eof = self.request_redraw_post_eof.saturating_add(1);
            }
            RedrawRequestSource::SmokePlayingResize => {
                self.request_redraw_smoke_playing_resize =
                    self.request_redraw_smoke_playing_resize.saturating_add(1);
            }
        }
    }

    pub fn record_scheduler_evaluation(&mut self, early_wait: bool) {
        self.scheduler_evaluations = self.scheduler_evaluations.saturating_add(1);
        if early_wait {
            self.scheduler_early_waits = self.scheduler_early_waits.saturating_add(1);
        }
    }

    pub fn record_control_flow_wait_until(&mut self, deadline: Instant, now: Instant) {
        if deadline > now {
            self.control_flow_wait_until_future =
                self.control_flow_wait_until_future.saturating_add(1);
        } else {
            self.control_flow_wait_until_expired =
                self.control_flow_wait_until_expired.saturating_add(1);
        }
    }

    pub fn record_control_flow_wait(&mut self) {
        self.control_flow_wait = self.control_flow_wait.saturating_add(1);
    }

    pub fn record_queue_submission(&mut self) {
        self.queue_submissions = self.queue_submissions.saturating_add(1);
    }

    pub fn record_surface_present(&mut self) {
        self.surface_presents = self.surface_presents.saturating_add(1);
    }

    pub fn record_egui_repaint_delay(&mut self, delay: Duration) {
        if delay.is_zero() {
            self.zero_egui_repaint_delays = self.zero_egui_repaint_delays.saturating_add(1);
        } else {
            self.non_zero_egui_repaint_delays = self.non_zero_egui_repaint_delays.saturating_add(1);
        }
    }

    pub fn print_summary(&self) {
        println!("=== Integration 8A loop metrics ===");
        println!("total_window_events: {}", self.total_window_events);
        println!("about_to_wait_calls: {}", self.about_to_wait_calls);
        println!("redraw_requested_events: {}", self.redraw_requested_events);
        println!("request_redraw_calls: {}", self.request_redraw_calls);
        println!(
            "request_redraw_sources: egui_event={} egui_zero={} resize={} surface_retry={} init={} post_eof={} smoke_resize={}",
            self.request_redraw_egui_event_response,
            self.request_redraw_egui_zero_delay,
            self.request_redraw_resize,
            self.request_redraw_surface_retry,
            self.request_redraw_initialization,
            self.request_redraw_post_eof,
            self.request_redraw_smoke_playing_resize,
        );
        println!("scheduler_evaluations: {}", self.scheduler_evaluations);
        println!("scheduler_early_waits: {}", self.scheduler_early_waits);
        println!(
            "control_flow: wait_until_future={} wait_until_expired={} wait={}",
            self.control_flow_wait_until_future,
            self.control_flow_wait_until_expired,
            self.control_flow_wait
        );
        println!("queue_submissions: {}", self.queue_submissions);
        println!("surface_presents: {}", self.surface_presents);
        println!(
            "egui_repaint_delay: zero={} non_zero={}",
            self.zero_egui_repaint_delays, self.non_zero_egui_repaint_delays
        );
    }
}
