//! UI intents and layout output produced during a frame.

use egui::Rect;

/// User-requested actions emitted by the editor shell.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum UiIntent {
    StartPlayback,
    CloseRequested,
}

/// Geometry and intents generated while laying out the editor shell.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorUiOutput {
    pub program_monitor_rect: Rect,
    pub intents: Vec<UiIntent>,
}

impl EditorUiOutput {
    /// Creates an empty output with a zero monitor rectangle.
    pub fn empty() -> Self {
        Self {
            program_monitor_rect: Rect::NOTHING,
            intents: Vec::new(),
        }
    }
}
