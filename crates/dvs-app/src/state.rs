//! Application state machine for the production video window.

use crate::error::AppError;

/// High-level application lifecycle state.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum AppState {
    Initializing,
    Ready,
    Playing,
    Ended,
    Closing,
    Fatal,
}

impl AppState {
    /// Returns whether playback work may still be scheduled.
    pub const fn allows_playback(self) -> bool {
        matches!(self, Self::Ready | Self::Playing | Self::Ended)
    }

    /// Returns whether the application is still live and may receive input.
    pub const fn is_live(self) -> bool {
        matches!(
            self,
            Self::Initializing | Self::Ready | Self::Playing | Self::Ended
        )
    }

    /// Startup completed successfully.
    pub const fn ready(self) -> Result<Self, AppError> {
        match self {
            Self::Initializing => Ok(Self::Ready),
            _ => Err(AppError::InvalidState),
        }
    }

    /// Starts PTS playback once from the ready state.
    pub const fn start_playback(self) -> Result<Self, AppError> {
        match self {
            Self::Ready => Ok(Self::Playing),
            Self::Playing | Self::Ended => Err(AppError::InvalidState),
            _ => Err(AppError::InvalidState),
        }
    }

    /// Marks playback as finished while keeping the last presented frame visible.
    pub const fn eof_reached(self) -> Result<Self, AppError> {
        match self {
            Self::Playing => Ok(Self::Ended),
            Self::Ended => Ok(Self::Ended),
            _ => Err(AppError::InvalidState),
        }
    }

    /// Returns whether resizing should preserve the current lifecycle state.
    pub const fn resize_preserves_state(self) -> bool {
        matches!(self, Self::Ready | Self::Playing | Self::Ended)
    }
    pub const fn begin_close(self) -> Self {
        match self {
            Self::Fatal | Self::Closing => self,
            _ => Self::Closing,
        }
    }

    /// Marks a fatal runtime error.
    pub const fn mark_fatal(self) -> Self {
        Self::Fatal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_to_ready() {
        assert_eq!(
            AppState::Initializing.ready().expect("ready"),
            AppState::Ready
        );
    }

    #[test]
    fn ready_to_playing_once() {
        assert_eq!(
            AppState::Ready.start_playback().expect("start"),
            AppState::Playing
        );
        assert!(AppState::Playing.start_playback().is_err());
        assert!(AppState::Ended.start_playback().is_err());
    }

    #[test]
    fn playing_to_ended() {
        assert_eq!(
            AppState::Playing.eof_reached().expect("eof"),
            AppState::Ended
        );
        assert_eq!(
            AppState::Ended.eof_reached().expect("eof again"),
            AppState::Ended
        );
    }

    #[test]
    fn resize_preserves_live_states() {
        for state in [AppState::Ready, AppState::Playing, AppState::Ended] {
            assert!(state.resize_preserves_state());
        }
        assert!(!AppState::Initializing.resize_preserves_state());
    }

    #[test]
    fn close_from_each_live_state() {
        for state in [
            AppState::Initializing,
            AppState::Ready,
            AppState::Playing,
            AppState::Ended,
        ] {
            assert_eq!(state.begin_close(), AppState::Closing);
        }
    }

    #[test]
    fn fatal_transition() {
        assert_eq!(AppState::Playing.mark_fatal(), AppState::Fatal);
    }
}
