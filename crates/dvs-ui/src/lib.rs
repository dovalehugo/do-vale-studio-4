#![forbid(unsafe_code)]

//! Platform-neutral editor UI definitions for Do Vale Studio 4.

mod editor;
mod intent;
mod layout;
mod model;
mod theme;
mod timecode;

pub use editor::EditorUi;
pub use intent::{EditorUiOutput, UiIntent};
pub use layout::{
    EditorLayout, LayoutMode, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, MONITOR_INNER_PADDING,
    compute_editor_layout, program_monitor_video_rect, transport_play_button_rect,
};
pub use model::{EditorPlaybackState, EditorViewModel};
pub use theme::apply_editor_theme;
pub use timecode::format_timecode;
