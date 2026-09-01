//! Deterministic PTS frame scheduler.

use dvs_media::{FrameId, MediaTimestamp};

use crate::error::PlaybackError;
use crate::time::{MediaTimeUs, normalize_timestamp};

/// Scheduler decision for one frame relative to monotonic playback time.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ScheduleDecision {
    /// Wait until the target media time is reached.
    WaitUntil {
        /// Media-time target relative to the first PTS.
        target: MediaTimeUs,
    },
    /// Present now; lateness may be zero or positive within the late-present band.
    PresentNow {
        /// Observed lateness in microseconds (zero when exactly on time).
        lateness: MediaTimeUs,
    },
    /// Drop before bridge preparation because the frame is irrecoverably late.
    DropLate {
        /// Observed lateness in microseconds.
        lateness: MediaTimeUs,
    },
    /// Reject the frame due to timestamp validation failure.
    RejectTimestamp(PlaybackError),
}

/// Immutable per-frame scheduling plan derived from PTS metadata.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FrameSchedulePlan {
    target: MediaTimeUs,
    is_first_frame: bool,
}

impl FrameSchedulePlan {
    /// Returns the media-time target for this frame.
    pub const fn target(self) -> MediaTimeUs {
        self.target
    }
}

/// Configuration for PTS scheduling thresholds.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SchedulerConfig {
    /// How early a frame may be presented before its target.
    pub early_tolerance_us: u64,
    /// Lateness beyond which a frame is dropped before bridge preparation.
    pub late_drop_threshold_us: u64,
}

impl SchedulerConfig {
    /// Creates a validated scheduler configuration.
    pub const fn new(early_tolerance_us: u64, late_drop_threshold_us: u64) -> Self {
        Self {
            early_tolerance_us,
            late_drop_threshold_us,
        }
    }

    /// Derives thresholds from an observed frame duration.
    ///
    /// Late-drop spans two frame durations.
    pub fn from_frame_duration(frame_duration_us: MediaTimeUs) -> Self {
        let frame = frame_duration_us.0.max(1) as u64;
        Self::new(0, frame.saturating_mul(2))
    }

    /// Default thresholds for NTSC 30000/1001 cadence (~33.366 ms per frame).
    pub const fn ntsc_30000_over_1001_default() -> Self {
        Self::new(0, 66_732)
    }
}

/// Deterministic frame scheduler using normalized PTS and monotonic elapsed time.
#[derive(Debug, Clone)]
pub struct FrameScheduler {
    config: SchedulerConfig,
    media_anchor: Option<MediaTimestamp>,
    previous_normalized_pts: Option<MediaTimeUs>,
    is_first_frame: bool,
}

impl FrameScheduler {
    /// Creates a scheduler with the given configuration.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            media_anchor: None,
            previous_normalized_pts: None,
            is_first_frame: true,
        }
    }

    /// Returns the active configuration.
    pub const fn config(&self) -> &SchedulerConfig {
        &self.config
    }

    /// Resets anchor state for a new playback pass.
    pub fn reset(&mut self) {
        self.media_anchor = None;
        self.previous_normalized_pts = None;
        self.is_first_frame = true;
    }

    /// Plans one frame from its timestamp. Call exactly once per decoded frame.
    pub fn plan_frame(
        &mut self,
        timestamp: Option<MediaTimestamp>,
        frame_id: FrameId,
    ) -> Result<FrameSchedulePlan, PlaybackError> {
        let _ = frame_id;
        let Some(timestamp) = timestamp else {
            return Err(PlaybackError::MissingTimestamp);
        };

        let target = self.normalize_and_validate(timestamp)?;
        let is_first_frame = self.is_first_frame;
        if is_first_frame {
            self.is_first_frame = false;
        }
        Ok(FrameSchedulePlan {
            target,
            is_first_frame,
        })
    }

    /// Evaluates a previously planned frame against elapsed monotonic playback time.
    ///
    /// Safe to call repeatedly while waiting for the frame target.
    pub fn evaluate_plan(&self, elapsed: MediaTimeUs, plan: FrameSchedulePlan) -> ScheduleDecision {
        self.decide(elapsed, plan.target, plan.is_first_frame)
    }

    /// Plans and immediately evaluates one frame. Intended for unit tests.
    pub fn schedule_frame(
        &mut self,
        elapsed: MediaTimeUs,
        timestamp: Option<MediaTimestamp>,
        frame_id: FrameId,
    ) -> ScheduleDecision {
        match self.plan_frame(timestamp, frame_id) {
            Ok(plan) => self.evaluate_plan(elapsed, plan),
            Err(error) => ScheduleDecision::RejectTimestamp(error),
        }
    }

    fn normalize_and_validate(
        &mut self,
        timestamp: MediaTimestamp,
    ) -> Result<MediaTimeUs, PlaybackError> {
        if let Some(anchor) = self.media_anchor {
            let normalized = normalize_timestamp(timestamp, anchor)?;
            if let Some(previous) = self.previous_normalized_pts
                && normalized <= previous
            {
                return Err(PlaybackError::NonMonotonicTimestamp);
            }
            self.previous_normalized_pts = Some(normalized);
            Ok(normalized)
        } else {
            self.media_anchor = Some(timestamp);
            self.previous_normalized_pts = Some(MediaTimeUs::ZERO);
            Ok(MediaTimeUs::ZERO)
        }
    }

    fn decide(
        &self,
        elapsed: MediaTimeUs,
        target: MediaTimeUs,
        is_first_frame: bool,
    ) -> ScheduleDecision {
        if is_first_frame {
            return ScheduleDecision::PresentNow {
                lateness: MediaTimeUs::ZERO,
            };
        }

        let early_boundary = target
            .0
            .saturating_sub(self.config.early_tolerance_us as i64);
        if elapsed.0 < early_boundary {
            return ScheduleDecision::WaitUntil { target };
        }

        let lateness = MediaTimeUs(elapsed.0.saturating_sub(target.0));
        if lateness.0 as u64 > self.config.late_drop_threshold_us {
            return ScheduleDecision::DropLate { lateness };
        }

        ScheduleDecision::PresentNow { lateness }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dvs_media::{FrameId, TimeBase};

    use crate::time::normalize_timestamp;

    fn ts(pts: i64) -> MediaTimestamp {
        MediaTimestamp::new(pts, TimeBase::new(1, 30_000).expect("tb"))
    }

    fn schedule(
        scheduler: &mut FrameScheduler,
        elapsed_us: i64,
        pts: i64,
        frame_id: u64,
    ) -> ScheduleDecision {
        scheduler.schedule_frame(
            MediaTimeUs(elapsed_us),
            Some(ts(pts)),
            FrameId::new(frame_id),
        )
    }

    #[test]
    fn repeated_evaluation_while_waiting_is_stable() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        let first = scheduler
            .plan_frame(Some(ts(0)), FrameId::new(0))
            .expect("plan first");
        assert_eq!(
            scheduler.evaluate_plan(MediaTimeUs(0), first),
            ScheduleDecision::PresentNow {
                lateness: MediaTimeUs::ZERO
            }
        );

        let second = scheduler
            .plan_frame(Some(ts(1001)), FrameId::new(1))
            .expect("plan second");
        assert_eq!(
            scheduler.evaluate_plan(MediaTimeUs(0), second),
            ScheduleDecision::WaitUntil {
                target: MediaTimeUs(33_366)
            }
        );
        assert_eq!(
            scheduler.evaluate_plan(MediaTimeUs(0), second),
            ScheduleDecision::WaitUntil {
                target: MediaTimeUs(33_366)
            }
        );
    }

    #[test]
    fn first_frame_due_immediately() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        assert_eq!(
            schedule(&mut scheduler, 0, 0, 0),
            ScheduleDecision::PresentNow {
                lateness: MediaTimeUs::ZERO
            }
        );
    }

    #[test]
    fn early_frame_waits() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        let _ = schedule(&mut scheduler, 0, 0, 0);
        assert_eq!(
            schedule(&mut scheduler, 0, 1001, 1),
            ScheduleDecision::WaitUntil {
                target: MediaTimeUs(33_366)
            }
        );
    }

    #[test]
    fn exactly_on_time_presents() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        let _ = schedule(&mut scheduler, 0, 0, 0);
        assert_eq!(
            schedule(&mut scheduler, 33_366, 1001, 1),
            ScheduleDecision::PresentNow {
                lateness: MediaTimeUs::ZERO
            }
        );
    }

    #[test]
    fn slightly_late_presents_and_records_lateness() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        let _ = schedule(&mut scheduler, 0, 0, 0);
        assert_eq!(
            schedule(&mut scheduler, 40_000, 1001, 1),
            ScheduleDecision::PresentNow {
                lateness: MediaTimeUs(6_634)
            }
        );
    }

    #[test]
    fn drop_threshold_crossing_drops() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        let _ = schedule(&mut scheduler, 0, 0, 0);
        assert!(matches!(
            schedule(&mut scheduler, 120_000, 1001, 1),
            ScheduleDecision::DropLate { .. }
        ));
    }

    #[test]
    fn exact_drop_threshold_boundary_presents() {
        let config = SchedulerConfig::new(0, 66_732);
        let mut scheduler = FrameScheduler::new(config);
        let _ = schedule(&mut scheduler, 0, 0, 0);
        assert_eq!(
            schedule(&mut scheduler, 33_366 + 66_732, 1001, 1),
            ScheduleDecision::PresentNow {
                lateness: MediaTimeUs(66_732)
            }
        );
    }

    #[test]
    fn one_microsecond_past_drop_threshold_drops() {
        let config = SchedulerConfig::new(0, 66_732);
        let mut scheduler = FrameScheduler::new(config);
        let _ = schedule(&mut scheduler, 0, 0, 0);
        assert!(matches!(
            schedule(&mut scheduler, 33_366 + 66_733, 1001, 1),
            ScheduleDecision::DropLate { .. }
        ));
    }

    #[test]
    fn ntsc_cadence_sequence() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        for frame in 0..5 {
            let pts = frame * 1001;
            let elapsed = normalize_timestamp(ts(pts), ts(0)).expect("normalize").0;
            let decision = schedule(&mut scheduler, elapsed, pts, frame as u64);
            assert!(
                matches!(decision, ScheduleDecision::PresentNow { .. }),
                "frame {frame}: {decision:?}"
            );
        }
    }

    #[test]
    fn non_zero_initial_pts_normalizes() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        assert_eq!(
            schedule(&mut scheduler, 0, 5_000, 0),
            ScheduleDecision::PresentNow {
                lateness: MediaTimeUs::ZERO
            }
        );
        let elapsed = normalize_timestamp(ts(5_000 + 1001), ts(5_000))
            .expect("normalize")
            .0;
        assert_eq!(
            schedule(&mut scheduler, elapsed, 5_000 + 1001, 1),
            ScheduleDecision::PresentNow {
                lateness: MediaTimeUs::ZERO
            }
        );
    }

    #[test]
    fn missing_pts_rejected() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        let decision = scheduler.schedule_frame(MediaTimeUs(0), None, FrameId::new(0));
        assert!(matches!(
            decision,
            ScheduleDecision::RejectTimestamp(PlaybackError::MissingTimestamp)
        ));
    }

    #[test]
    fn non_monotonic_pts_rejected() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        let _ = schedule(&mut scheduler, 0, 0, 0);
        let decision = schedule(&mut scheduler, 33_366, 0, 1);
        assert!(matches!(
            decision,
            ScheduleDecision::RejectTimestamp(PlaybackError::NonMonotonicTimestamp)
        ));
    }

    #[test]
    fn long_duration_does_not_drift_from_pts() {
        let mut scheduler = FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default());
        let frame_count = 300i64;
        for frame in 0..frame_count {
            let pts = frame * 1001;
            let elapsed = normalize_timestamp(ts(pts), ts(0)).expect("normalize").0;
            let decision = schedule(&mut scheduler, elapsed, pts, frame as u64);
            assert!(
                matches!(decision, ScheduleDecision::PresentNow { .. }),
                "frame {frame}: {decision:?}"
            );
        }
    }
}
