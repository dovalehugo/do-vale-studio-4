//! Bounded playback metrics.

use dvs_media::FrameId;

use crate::time::MediaTimeUs;

/// Aggregated playback metrics with bounded storage.
///
/// "Presented" means submitted and presented to the swapchain, not measured physical
/// monitor scanout.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct PlaybackMetrics {
    frames_decoded: u64,
    frames_scheduled: u64,
    frames_presented: u64,
    frames_dropped_late: u64,
    frames_rejected_timestamp: u64,
    early_wait_count: u64,
    late_present_count: u64,
    max_lateness_us: u64,
    cumulative_lateness_us: u128,
    first_presented_frame_id: Option<FrameId>,
    last_presented_frame_id: Option<FrameId>,
    playback_media_duration_us: Option<MediaTimeUs>,
    monotonic_wall_duration_us: Option<MediaTimeUs>,
    eof_reached: bool,
    surface_reconfigurations: u64,
}

impl PlaybackMetrics {
    /// Creates an empty metrics snapshot.
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn frames_decoded(&self) -> u64 {
        self.frames_decoded
    }

    pub const fn frames_scheduled(&self) -> u64 {
        self.frames_scheduled
    }

    pub const fn frames_presented(&self) -> u64 {
        self.frames_presented
    }

    pub const fn frames_dropped_late(&self) -> u64 {
        self.frames_dropped_late
    }

    pub const fn frames_rejected_timestamp(&self) -> u64 {
        self.frames_rejected_timestamp
    }

    pub const fn early_wait_count(&self) -> u64 {
        self.early_wait_count
    }

    pub const fn late_present_count(&self) -> u64 {
        self.late_present_count
    }

    pub const fn max_lateness_us(&self) -> u64 {
        self.max_lateness_us
    }

    pub const fn cumulative_lateness_us(&self) -> u128 {
        self.cumulative_lateness_us
    }

    pub fn average_lateness_us(&self) -> u64 {
        if self.late_present_count == 0 {
            0
        } else {
            (self.cumulative_lateness_us / u128::from(self.late_present_count)) as u64
        }
    }

    pub const fn first_presented_frame_id(&self) -> Option<FrameId> {
        self.first_presented_frame_id
    }

    pub const fn last_presented_frame_id(&self) -> Option<FrameId> {
        self.last_presented_frame_id
    }

    pub const fn playback_media_duration_us(&self) -> Option<MediaTimeUs> {
        self.playback_media_duration_us
    }

    pub const fn monotonic_wall_duration_us(&self) -> Option<MediaTimeUs> {
        self.monotonic_wall_duration_us
    }

    pub const fn eof_reached(&self) -> bool {
        self.eof_reached
    }

    pub const fn surface_reconfigurations(&self) -> u64 {
        self.surface_reconfigurations
    }

    pub fn record_decoded(&mut self) {
        self.frames_decoded = self.frames_decoded.saturating_add(1);
    }

    pub fn record_scheduled(&mut self) {
        self.frames_scheduled = self.frames_scheduled.saturating_add(1);
    }

    pub fn record_early_wait(&mut self) {
        self.early_wait_count = self.early_wait_count.saturating_add(1);
    }

    pub fn record_dropped_late(&mut self) {
        self.frames_dropped_late = self.frames_dropped_late.saturating_add(1);
    }

    pub fn record_rejected_timestamp(&mut self) {
        self.frames_rejected_timestamp = self.frames_rejected_timestamp.saturating_add(1);
    }

    pub fn record_presented(&mut self, frame_id: FrameId, lateness: MediaTimeUs) {
        self.frames_presented = self.frames_presented.saturating_add(1);
        if self.first_presented_frame_id.is_none() {
            self.first_presented_frame_id = Some(frame_id);
        }
        self.last_presented_frame_id = Some(frame_id);
        if lateness.0 > 0 {
            self.late_present_count = self.late_present_count.saturating_add(1);
            let lateness_us = lateness.0.max(0) as u64;
            self.max_lateness_us = self.max_lateness_us.max(lateness_us);
            self.cumulative_lateness_us = self
                .cumulative_lateness_us
                .saturating_add(u128::from(lateness_us));
        }
    }

    pub fn record_surface_reconfiguration(&mut self) {
        self.surface_reconfigurations = self.surface_reconfigurations.saturating_add(1);
    }

    pub fn record_eof(&mut self, media_duration: Option<MediaTimeUs>, wall_duration: MediaTimeUs) {
        self.eof_reached = true;
        self.playback_media_duration_us = media_duration;
        self.monotonic_wall_duration_us = Some(wall_duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_lateness_uses_overflow_safe_arithmetic() {
        let mut metrics = PlaybackMetrics::new();
        metrics.record_presented(FrameId::new(0), MediaTimeUs(1_000));
        metrics.record_presented(FrameId::new(1), MediaTimeUs(3_000));
        assert_eq!(metrics.late_present_count(), 2);
        assert_eq!(metrics.average_lateness_us(), 2_000);
        assert_eq!(metrics.max_lateness_us(), 3_000);
    }

    #[test]
    fn eof_records_duration_fields() {
        let mut metrics = PlaybackMetrics::new();
        metrics.record_eof(Some(MediaTimeUs(3_000_000)), MediaTimeUs(3_100_000));
        assert!(metrics.eof_reached());
        assert_eq!(
            metrics.playback_media_duration_us(),
            Some(MediaTimeUs(3_000_000))
        );
        assert_eq!(
            metrics.monotonic_wall_duration_us(),
            Some(MediaTimeUs(3_100_000))
        );
    }
}
