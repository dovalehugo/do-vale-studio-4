//! Responsive editor layout geometry.

use egui::{Pos2, Rect, Vec2};

/// Minimum usable editor window size in logical points.
pub const MIN_WINDOW_WIDTH: f32 = 960.0;
pub const MIN_WINDOW_HEIGHT: f32 = 600.0;

const TOP_BAR_HEIGHT: f32 = 46.0;
const TRANSPORT_HEIGHT: f32 = 48.0;
const TIMELINE_HEIGHT: f32 = 240.0;
const MEDIA_PANEL_WIDTH: f32 = 260.0;
const INSPECTOR_PANEL_WIDTH: f32 = 300.0;
const COMPACT_BREAKPOINT_WIDTH: f32 = 1100.0;
const MIN_MONITOR_WIDTH: f32 = 320.0;
const MIN_MONITOR_HEIGHT: f32 = 180.0;
const MIN_SIDE_PANEL_WIDTH: f32 = 180.0;

/// Inner padding between Program Monitor chrome and the transparent video hole.
pub const MONITOR_INNER_PADDING: f32 = 8.0;

/// Layout density mode derived from the available window size.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LayoutMode {
    Full,
    Compact,
}

/// Computed editor layout regions in logical points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorLayout {
    pub top_bar: Rect,
    pub media_panel: Rect,
    pub inspector_panel: Rect,
    pub program_monitor: Rect,
    pub transport: Rect,
    pub timeline: Rect,
    pub mode: LayoutMode,
}

/// Computes editor layout regions for the given available size.
pub fn compute_editor_layout(available: Vec2) -> EditorLayout {
    let width = available.x.max(MIN_WINDOW_WIDTH);
    let height = available.y.max(MIN_WINDOW_HEIGHT);
    let mode = if width < COMPACT_BREAKPOINT_WIDTH {
        LayoutMode::Compact
    } else {
        LayoutMode::Full
    };

    let top_bar = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, TOP_BAR_HEIGHT));
    let timeline = Rect::from_min_size(
        Pos2::new(0.0, height - TIMELINE_HEIGHT),
        Vec2::new(width, TIMELINE_HEIGHT),
    );
    let transport = Rect::from_min_size(
        Pos2::new(0.0, timeline.min.y - TRANSPORT_HEIGHT),
        Vec2::new(width, TRANSPORT_HEIGHT),
    );

    let center_top = top_bar.max.y;
    let center_bottom = transport.min.y;
    let center_height = (center_bottom - center_top).max(MIN_MONITOR_HEIGHT);

    let mut media_width = if mode == LayoutMode::Compact {
        MIN_SIDE_PANEL_WIDTH
    } else {
        MEDIA_PANEL_WIDTH
    };
    let mut inspector_width = if mode == LayoutMode::Compact {
        MIN_SIDE_PANEL_WIDTH
    } else {
        INSPECTOR_PANEL_WIDTH
    };

    let max_side_total = (width - MIN_MONITOR_WIDTH).max(0.0);
    let side_total = media_width + inspector_width;
    if side_total > max_side_total && max_side_total > 0.0 {
        let scale = max_side_total / side_total;
        media_width = (media_width * scale).max(MIN_SIDE_PANEL_WIDTH.min(max_side_total * 0.5));
        inspector_width =
            (inspector_width * scale).max(MIN_SIDE_PANEL_WIDTH.min(max_side_total * 0.5));
        let adjusted_total = media_width + inspector_width;
        if adjusted_total > max_side_total {
            media_width = max_side_total * 0.5;
            inspector_width = max_side_total - media_width;
        }
    }

    let monitor_width = (width - media_width - inspector_width).max(MIN_MONITOR_WIDTH);
    let media_panel = Rect::from_min_size(
        Pos2::new(0.0, center_top),
        Vec2::new(media_width, center_height),
    );
    let program_monitor = Rect::from_min_size(
        Pos2::new(media_width, center_top),
        Vec2::new(monitor_width, center_height),
    );
    let inspector_panel = Rect::from_min_size(
        Pos2::new(media_width + monitor_width, center_top),
        Vec2::new(inspector_width, center_height),
    );

    EditorLayout {
        top_bar,
        media_panel,
        inspector_panel,
        program_monitor,
        transport,
        timeline,
        mode,
    }
}

/// Returns the transparent inner rectangle where GPU video is composed.
pub fn program_monitor_video_rect(layout: EditorLayout) -> Rect {
    layout
        .program_monitor
        .shrink2(Vec2::splat(MONITOR_INNER_PADDING))
}

/// Returns the interactive Play button rectangle inside the transport bar.
pub fn transport_play_button_rect(transport: Rect) -> Rect {
    let inner = transport.shrink2(Vec2::new(12.0, 8.0));
    Rect::from_min_size(inner.left_top(), Vec2::new(34.0, 30.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_window_keeps_monitor_non_zero() {
        let layout = compute_editor_layout(Vec2::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT));
        assert!(layout.program_monitor.width() >= MIN_MONITOR_WIDTH);
        assert!(layout.program_monitor.height() >= MIN_MONITOR_HEIGHT);
    }

    #[test]
    fn compact_mode_at_narrow_width() {
        let layout = compute_editor_layout(Vec2::new(1000.0, 700.0));
        assert_eq!(layout.mode, LayoutMode::Compact);
    }

    #[test]
    fn full_mode_at_wide_width() {
        let layout = compute_editor_layout(Vec2::new(1600.0, 900.0));
        assert_eq!(layout.mode, LayoutMode::Full);
    }

    #[test]
    fn monitor_rect_is_finite() {
        let layout = compute_editor_layout(Vec2::new(1280.0, 720.0));
        assert!(layout.program_monitor.is_positive());
        assert!(layout.program_monitor.width().is_finite());
        assert!(layout.program_monitor.height().is_finite());
    }

    #[test]
    fn video_rect_is_smaller_than_monitor_chrome() {
        let layout = compute_editor_layout(Vec2::new(1280.0, 720.0));
        let video = program_monitor_video_rect(layout);
        assert!(video.width() < layout.program_monitor.width());
        assert!(video.height() < layout.program_monitor.height());
    }

    #[test]
    fn transport_play_button_is_inside_transport() {
        let layout = compute_editor_layout(Vec2::new(1280.0, 720.0));
        let play = transport_play_button_rect(layout.transport);
        assert!(layout.transport.contains_rect(play));
    }
}
