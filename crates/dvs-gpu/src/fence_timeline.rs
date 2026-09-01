//! Monotonic shared-fence value generation for continuous playback.
//!
//! This module calculates fence values only. It does not perform GPU wait or signal
//! operations. Integration 3 wires these values to D3D11/D3D12 fence primitives.

use crate::error::GpuError;

/// Fence values for a single frame on the continuous-playback timeline.
///
/// Sequence per frame index `N`:
/// - `wait_consumed`: `None` for `N = 0`, otherwise `Some(2N)`
/// - `ready`: `2N + 1`
/// - `consumed`: `2N + 2`
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct FrameFenceValues {
    frame_index: u64,
    wait_consumed: Option<u64>,
    ready: u64,
    consumed: u64,
}

impl FrameFenceValues {
    /// Returns the frame index these values belong to.
    pub fn frame_index(self) -> u64 {
        self.frame_index
    }

    /// Returns the previous frame's consumed value to wait on before reuse, if any.
    pub fn wait_consumed(self) -> Option<u64> {
        self.wait_consumed
    }

    /// Returns the ready signal value for this frame.
    pub fn ready(self) -> u64 {
        self.ready
    }

    /// Returns the consumed signal value after presentation for this frame.
    pub fn consumed(self) -> u64 {
        self.consumed
    }
}

/// Generates monotonic fence values matching GPU Experiment 2 continuous playback.
///
/// Values are calculated in safe Rust only. No D3D fence objects are created here.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct FenceTimeline {
    frame_index: u64,
}

impl FenceTimeline {
    /// Creates a timeline positioned at frame 0.
    pub fn new() -> Self {
        Self { frame_index: 0 }
    }

    /// Returns fence values for the current frame without advancing.
    pub fn current(&self) -> Result<FrameFenceValues, GpuError> {
        fence_values_for_frame(self.frame_index)
    }

    /// Advances the timeline by exactly one frame.
    pub fn advance(&mut self) -> Result<(), GpuError> {
        let next = self
            .frame_index
            .checked_add(1)
            .ok_or(GpuError::TimelineExhausted)?;
        fence_values_for_frame(next)?;
        self.frame_index = next;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn at_frame(frame_index: u64) -> Self {
        Self { frame_index }
    }
}

impl Default for FenceTimeline {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn fence_values_for_frame(frame_index: u64) -> Result<FrameFenceValues, GpuError> {
    let doubled = frame_index
        .checked_mul(2)
        .ok_or(GpuError::TimelineExhausted)?;
    let ready = doubled.checked_add(1).ok_or(GpuError::TimelineExhausted)?;
    let consumed = doubled.checked_add(2).ok_or(GpuError::TimelineExhausted)?;
    let wait_consumed = if frame_index == 0 {
        None
    } else {
        Some(doubled)
    };

    Ok(FrameFenceValues {
        frame_index,
        wait_consumed,
        ready,
        consumed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_frame_0_values() {
        let timeline = FenceTimeline::new();
        let values = timeline.current().expect("frame 0");
        assert_eq!(values.frame_index(), 0);
        assert_eq!(values.wait_consumed(), None);
        assert_eq!(values.ready(), 1);
        assert_eq!(values.consumed(), 2);
    }

    #[test]
    fn timeline_frame_1_values() {
        let mut timeline = FenceTimeline::new();
        timeline.advance().expect("advance to 1");
        let values = timeline.current().expect("frame 1");
        assert_eq!(values.frame_index(), 1);
        assert_eq!(values.wait_consumed(), Some(2));
        assert_eq!(values.ready(), 3);
        assert_eq!(values.consumed(), 4);
    }

    #[test]
    fn timeline_frame_89_values() {
        let timeline = FenceTimeline::at_frame(89);
        let values = timeline.current().expect("frame 89");
        assert_eq!(values.ready(), 179);
        assert_eq!(values.consumed(), 180);
    }

    #[test]
    fn consecutive_frames_never_reuse_values() {
        let mut timeline = FenceTimeline::new();
        let mut seen = Vec::new();
        for _ in 0..16 {
            let values = timeline.current().expect("values");
            assert!(!seen.contains(&values.ready()));
            assert!(!seen.contains(&values.consumed()));
            seen.push(values.ready());
            seen.push(values.consumed());
            timeline.advance().expect("advance");
        }
    }

    #[test]
    fn timeline_exhaustion_returns_typed_error_without_wrapping() {
        let last_valid = (u64::MAX - 2) / 2;
        fence_values_for_frame(last_valid).expect("last valid frame");
        assert!(matches!(
            fence_values_for_frame(last_valid + 1).unwrap_err(),
            GpuError::TimelineExhausted
        ));

        let mut timeline = FenceTimeline::at_frame(last_valid);
        assert!(matches!(
            timeline.advance().unwrap_err(),
            GpuError::TimelineExhausted
        ));
    }

    #[test]
    fn frame_fence_values_are_copy_and_eq() {
        let a = fence_values_for_frame(3).expect("values");
        let b = a;
        assert_eq!(a, b);
        let _ = a.ready();
        let _ = b.ready();
    }
}
