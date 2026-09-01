//! PTS-driven continuous playback scheduling for Do Vale Studio 4.
//!
//! Platform-neutral clock, scheduler, and metrics live here. Windows hardware
//! validation binaries depend on decoder, GPU, and render crates as dev-dependencies
//! only.

#![forbid(unsafe_code)]

mod clock;
mod error;
mod metrics;
mod scheduler;
mod time;

pub use clock::{PlaybackClock, PlaybackState};
pub use error::PlaybackError;
pub use metrics::PlaybackMetrics;
pub use scheduler::{FrameSchedulePlan, FrameScheduler, ScheduleDecision, SchedulerConfig};
pub use time::{
    MediaTimeUs, media_duration_between, normalize_timestamp, ntsc_30000_over_1001_time_base,
    timestamp_to_microseconds,
};

#[cfg(test)]
mod send_sync {
    use std::sync::{Arc, Mutex};

    use super::*;

    const fn assert_send_sync<T: Send + Sync>() {}

    const _: () = {
        assert_send_sync::<PlaybackError>();
        assert_send_sync::<MediaTimeUs>();
        assert_send_sync::<SchedulerConfig>();
        assert_send_sync::<ScheduleDecision>();
        assert_send_sync::<PlaybackMetrics>();
        assert_send_sync::<PlaybackState>();
    };

    #[test]
    fn playback_value_types_are_send_and_sync() {
        fn assert_values<T: Send + Sync>(value: T) {
            let _ = Arc::new(Mutex::new(value));
        }

        assert_values(PlaybackError::MissingTimestamp);
        assert_values(MediaTimeUs::ZERO);
        assert_values(SchedulerConfig::ntsc_30000_over_1001_default());
        assert_values(ScheduleDecision::PresentNow {
            lateness: MediaTimeUs::ZERO,
        });
        assert_values(PlaybackMetrics::new());
        assert_values(PlaybackState::Stopped);
    }
}
