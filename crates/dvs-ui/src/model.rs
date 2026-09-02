//! View-model data passed from the application into the editor UI.

/// Playback lifecycle exposed to the editor shell.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum EditorPlaybackState {
    Initializing,
    Ready,
    Playing,
    Ended,
}

impl EditorPlaybackState {
    /// Returns whether the Play control may start playback.
    pub const fn play_enabled(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Returns whether the Play control should appear disabled.
    pub const fn play_disabled(self) -> bool {
        matches!(self, Self::Playing | Self::Ended)
    }
}

/// Application-owned data rendered by the editor shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorViewModel {
    pub app_title: String,
    pub media_name: String,
    pub playback_state: EditorPlaybackState,
    pub current_time_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub frame_id: Option<u64>,
    pub status_text: String,
}

impl EditorViewModel {
    /// Returns a human-readable playback-state label.
    pub fn playback_state_label(&self) -> &'static str {
        match self.playback_state {
            EditorPlaybackState::Initializing => "Initializing",
            EditorPlaybackState::Ready => "Ready",
            EditorPlaybackState::Playing => "Playing",
            EditorPlaybackState::Ended => "Ended",
        }
    }
}
