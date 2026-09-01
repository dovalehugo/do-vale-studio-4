//! Monotonic playback clock anchored to the first media PTS.

use std::time::{Duration, Instant};

use dvs_media::MediaTimestamp;

use crate::error::PlaybackError;
use crate::time::{MediaTimeUs, normalize_timestamp};

/// Playback transport state.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum PlaybackState {
    /// Playback has not started or was reset.
    #[default]
    Stopped,
    /// Playback is active.
    Playing,
    /// End of stream reached and playback finished.
    Ended,
}

/// Monotonic host-time playback clock with a single 1.0x rate.
///
/// Uses [`Instant`] (monotonic, not wall clock). Resize and redraw events do not
/// reset the clock.
#[derive(Debug, Clone)]
pub struct PlaybackClock {
    state: PlaybackState,
    media_anchor: Option<MediaTimestamp>,
    host_start: Option<Instant>,
}

impl Default for PlaybackClock {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaybackClock {
    /// Creates a stopped clock with no media anchor.
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
            media_anchor: None,
            host_start: None,
        }
    }

    /// Returns the current playback state.
    pub const fn state(&self) -> PlaybackState {
        self.state
    }

    /// Returns the first valid media PTS anchor once playback has started.
    pub const fn media_anchor(&self) -> Option<MediaTimestamp> {
        self.media_anchor
    }

    /// Starts playback using the first frame timestamp as the media anchor.
    ///
    /// The first frame is due immediately (`MediaTimeUs::ZERO`).
    pub fn start(&mut self, first_timestamp: MediaTimestamp) -> Result<(), PlaybackError> {
        if self.state == PlaybackState::Playing {
            return Err(PlaybackError::InvalidClockState);
        }
        self.media_anchor = Some(first_timestamp);
        self.host_start = Some(Instant::now());
        self.state = PlaybackState::Playing;
        Ok(())
    }

    /// Marks playback as ended while retaining timing history for metrics.
    pub fn mark_ended(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Ended;
        }
    }

    /// Returns elapsed monotonic host time since playback started.
    pub fn elapsed_host(&self) -> Option<Duration> {
        self.host_start.map(|start| start.elapsed())
    }

    /// Returns elapsed monotonic host time as microseconds.
    pub fn elapsed_host_us(&self) -> Option<MediaTimeUs> {
        self.elapsed_host()
            .map(|duration| MediaTimeUs(i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)))
    }

    /// Returns the current media position relative to the anchor.
    pub fn current_media_position(&self) -> Option<MediaTimeUs> {
        let elapsed = self.elapsed_host_us()?;
        Some(elapsed)
    }

    /// Normalizes a frame timestamp against the established media anchor.
    pub fn normalize_frame_timestamp(
        &self,
        timestamp: MediaTimestamp,
    ) -> Result<MediaTimeUs, PlaybackError> {
        let anchor = self.media_anchor.ok_or(PlaybackError::InvalidClockState)?;
        normalize_timestamp(timestamp, anchor)
    }

    /// Returns the monotonic instant when a normalized media target becomes due.
    pub fn host_instant_for_media_target(
        &self,
        target: MediaTimeUs,
    ) -> Result<Instant, PlaybackError> {
        let start = self.host_start.ok_or(PlaybackError::InvalidClockState)?;
        let delay = Duration::from_micros(target.0.max(0) as u64);
        Ok(start + delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dvs_media::TimeBase;
    use std::thread;

    #[test]
    fn first_frame_due_immediately_after_start() {
        let tb = TimeBase::new(1, 30_000).expect("tb");
        let first = MediaTimestamp::new(0, tb);
        let mut clock = PlaybackClock::new();
        clock.start(first).expect("start");
        let target = clock
            .normalize_frame_timestamp(first)
            .expect("normalize first");
        assert_eq!(target, MediaTimeUs::ZERO);
        let elapsed = clock.elapsed_host_us().expect("elapsed");
        assert!(elapsed.0 >= 0);
    }

    #[test]
    fn clock_advances_with_host_time() {
        let tb = TimeBase::new(1, 30_000).expect("tb");
        let first = MediaTimestamp::new(0, tb);
        let mut clock = PlaybackClock::new();
        clock.start(first).expect("start");
        thread::sleep(Duration::from_millis(2));
        let elapsed = clock.elapsed_host_us().expect("elapsed");
        assert!(elapsed.0 >= 1_000);
    }

    #[test]
    fn ended_state_retains_anchor() {
        let tb = TimeBase::new(1, 30_000).expect("tb");
        let first = MediaTimestamp::new(0, tb);
        let mut clock = PlaybackClock::new();
        clock.start(first).expect("start");
        clock.mark_ended();
        assert_eq!(clock.state(), PlaybackState::Ended);
        assert_eq!(clock.media_anchor(), Some(first));
    }
}
