//! Safe UI definitions for Do Vale Studio 4.
//!
//! This crate owns egui widgets, palette, and layout only. It must not own
//! windows, surfaces, GPU devices, decoders, bridges, or playback scheduling.

#![forbid(unsafe_code)]

/// Pure UI intent emitted by the editor shell. Never executes playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorAction {
    /// Request the host to start PTS playback once (same path as SPACE).
    StartPlayback,
}

/// Result of painting one editor-shell frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorShellOutput {
    /// Useful video area only (excludes Program Monitor chrome header).
    pub program_monitor: LogicalRect,
    pub action: Option<EditorAction>,
}

/// Logical (egui points) axis-aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Physical pixel viewport clamped to a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalViewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Shell metrics in egui points.
pub const TOP_BAR_HEIGHT: f32 = 32.0;
pub const TIMELINE_HEIGHT: f32 = 168.0;
pub const MEDIA_POOL_WIDTH: f32 = 220.0;
pub const INSPECTOR_WIDTH: f32 = 248.0;
/// Program Monitor chrome header height (outside the video rect).
pub const PROGRAM_MONITOR_HEADER_HEIGHT: f32 = 24.0;
pub const PANEL_SECTION_HEADER_HEIGHT: f32 = 22.0;
const PANEL_INNER_MARGIN: i8 = 8;
const TRACK_ROW_HEIGHT: f32 = 36.0;

/// Integration 8A.4 editor palette.
pub const COLOR_BG: egui::Color32 = egui::Color32::from_rgb(0x0F, 0x11, 0x15);
pub const COLOR_PANEL: egui::Color32 = egui::Color32::from_rgb(0x15, 0x18, 0x1D);
pub const COLOR_HEADER: egui::Color32 = egui::Color32::from_rgb(0x1B, 0x1F, 0x26);
pub const COLOR_BORDER: egui::Color32 = egui::Color32::from_rgb(0x29, 0x2F, 0x38);
pub const COLOR_TEXT: egui::Color32 = egui::Color32::from_rgb(0xE7, 0xEB, 0xF2);
pub const COLOR_TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x92, 0x9B, 0xAA);
pub const COLOR_ACCENT: egui::Color32 = egui::Color32::from_rgb(0x4D, 0xA3, 0xFF);
pub const COLOR_MONITOR: egui::Color32 = egui::Color32::from_rgb(0x05, 0x06, 0x07);

const CORNER: u8 = 2;

/// Maps a Play button click to a pure UI action (no playback side effects).
pub fn play_button_action(clicked: bool) -> Option<EditorAction> {
    clicked.then_some(EditorAction::StartPlayback)
}

/// Consumes a pending editor action exactly once.
pub fn take_editor_action(pending: &mut Option<EditorAction>) -> Option<EditorAction> {
    pending.take()
}

/// Installs the Do Vale Studio 4 editor visuals once on an egui context.
///
/// Safe to call multiple times; intended to run at overlay construction, not per frame.
pub fn apply_editor_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.window_fill = COLOR_BG;
    visuals.panel_fill = COLOR_PANEL;
    visuals.faint_bg_color = COLOR_HEADER;
    visuals.extreme_bg_color = COLOR_MONITOR;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, COLOR_BORDER);
    visuals.window_corner_radius = CORNER.into();
    visuals.menu_corner_radius = CORNER.into();
    visuals.popup_shadow = egui::Shadow::NONE;
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.override_text_color = Some(COLOR_TEXT);
    visuals.selection.bg_fill = COLOR_ACCENT.linear_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, COLOR_ACCENT);
    visuals.hyperlink_color = COLOR_ACCENT;
    visuals.widgets.noninteractive.bg_fill = COLOR_PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = COLOR_HEADER;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, COLOR_BORDER);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, COLOR_TEXT_MUTED);
    visuals.widgets.noninteractive.corner_radius = CORNER.into();
    visuals.widgets.inactive.bg_fill = COLOR_HEADER;
    visuals.widgets.inactive.weak_bg_fill = COLOR_HEADER;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, COLOR_BORDER);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, COLOR_TEXT);
    visuals.widgets.inactive.corner_radius = CORNER.into();
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x24, 0x2A, 0x33);
    visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(0x24, 0x2A, 0x33);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, COLOR_ACCENT);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, COLOR_TEXT);
    visuals.widgets.hovered.corner_radius = CORNER.into();
    visuals.widgets.active.bg_fill = COLOR_ACCENT.linear_multiply(0.25);
    visuals.widgets.active.weak_bg_fill = COLOR_ACCENT.linear_multiply(0.25);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, COLOR_ACCENT);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, COLOR_TEXT);
    visuals.widgets.active.corner_radius = CORNER.into();
    visuals.widgets.open.bg_fill = COLOR_HEADER;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, COLOR_BORDER);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, COLOR_TEXT);
    visuals.widgets.open.corner_radius = CORNER.into();
    ctx.set_visuals(visuals);
}

fn panel_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(COLOR_PANEL)
        .stroke(egui::Stroke::new(1.0_f32, COLOR_BORDER))
        .inner_margin(egui::Margin::ZERO)
}

fn top_bar_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(COLOR_HEADER)
        .stroke(egui::Stroke::new(1.0_f32, COLOR_BORDER))
        .inner_margin(egui::Margin::symmetric(10, 0))
}

fn label(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(size).color(color)).selectable(false));
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, PANEL_SECTION_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, COLOR_HEADER);
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x, rect.max.y - 0.5),
            egui::pos2(rect.max.x, rect.max.y - 0.5),
        ],
        egui::Stroke::new(1.0_f32, COLOR_BORDER),
    );
    ui.painter().text(
        egui::pos2(rect.min.x + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(11.0),
        COLOR_TEXT_MUTED,
    );
}

fn muted_badge(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(COLOR_BG)
        .stroke(egui::Stroke::new(1.0_f32, COLOR_BORDER))
        .corner_radius(CORNER)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            label(ui, text, 10.0, COLOR_TEXT_MUTED);
        });
}

fn paint_top_bar(ui: &mut egui::Ui) -> Option<EditorAction> {
    let mut action = None;
    ui.horizontal_centered(|ui| {
        label(ui, "DO VALE STUDIO 4", 13.0, COLOR_TEXT);
        ui.add_space(10.0);
        muted_badge(ui, "EDIT");
        ui.add_space(14.0);

        let play = egui::Button::new(egui::RichText::new("▶ Play").size(12.0).color(COLOR_TEXT))
            .fill(COLOR_BG)
            .stroke(egui::Stroke::new(1.0_f32, COLOR_ACCENT))
            .corner_radius(CORNER)
            .min_size(egui::vec2(72.0, 22.0));
        if ui.add(play).clicked() {
            action = play_button_action(true);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            muted_badge(ui, "GPU PREVIEW");
        });
    });
    action
}

fn paint_media_pool(ui: &mut egui::Ui) {
    section_header(ui, "MEDIA POOL");
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        label(ui, "Project Media", 12.0, COLOR_TEXT);
    });
    ui.add_space(4.0);
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(1.0_f32, COLOR_BORDER),
    );
    ui.add_space(1.0);

    let remaining = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(remaining, egui::Sense::hover());
    ui.painter().text(
        rect.center() - egui::vec2(0.0, 8.0),
        egui::Align2::CENTER_CENTER,
        "No media in project",
        egui::FontId::proportional(12.0),
        COLOR_TEXT_MUTED,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        "Media will appear here",
        egui::FontId::proportional(11.0),
        COLOR_TEXT_MUTED.linear_multiply(0.85),
    );
}

fn inspector_row(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        let name_width = 72.0;
        ui.add_sized(
            egui::vec2(name_width, 16.0),
            egui::Label::new(egui::RichText::new(name).size(11.0).color(COLOR_TEXT_MUTED))
                .selectable(false),
        );
        label(ui, value, 11.0, COLOR_TEXT);
    });
    ui.add_space(4.0);
}

fn paint_inspector(ui: &mut egui::Ui) {
    section_header(ui, "INSPECTOR");
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        label(ui, "No clip selected", 12.0, COLOR_TEXT_MUTED);
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        label(ui, "VIDEO", 11.0, COLOR_TEXT);
    });
    ui.add_space(6.0);
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(1.0_f32, COLOR_BORDER),
    );
    ui.add_space(8.0);
    inspector_row(ui, "Position", "—");
    inspector_row(ui, "Scale", "—");
    inspector_row(ui, "Rotation", "—");
    inspector_row(ui, "Opacity", "—");
}

fn paint_timeline(ui: &mut egui::Ui) {
    section_header(ui, "TIMELINE");
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        label(ui, "Empty sequence", 11.0, COLOR_TEXT_MUTED);
    });
    ui.add_space(8.0);

    let top = ui.cursor().top();
    let width = ui.available_width().max(1.0);
    let left = ui.min_rect().min.x + f32::from(PANEL_INNER_MARGIN);
    let usable_w = (width - f32::from(PANEL_INNER_MARGIN) * 2.0).max(1.0);

    // Shift painter origin by using absolute coords from ui.min_rect.
    let track_x = left;
    let v1_y = top;
    let a1_y = top + TRACK_ROW_HEIGHT + 4.0;

    let v1 = egui::Rect::from_min_size(
        egui::pos2(track_x, v1_y),
        egui::vec2(usable_w, TRACK_ROW_HEIGHT),
    );
    let a1 = egui::Rect::from_min_size(
        egui::pos2(track_x, a1_y),
        egui::vec2(usable_w, TRACK_ROW_HEIGHT),
    );

    for (name, rect) in [("V1", v1), ("A1", a1)] {
        let header_w = 40.0;
        let header = egui::Rect::from_min_size(rect.min, egui::vec2(header_w, rect.height()));
        let lane =
            egui::Rect::from_min_max(egui::pos2(rect.min.x + header_w, rect.min.y), rect.max);
        ui.painter()
            .rect_filled(header, egui::CornerRadius::ZERO, COLOR_HEADER);
        ui.painter()
            .rect_filled(lane, egui::CornerRadius::ZERO, COLOR_BG);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.0_f32, COLOR_BORDER),
            egui::StrokeKind::Inside,
        );
        ui.painter().line_segment(
            [
                egui::pos2(rect.min.x + header_w, rect.min.y),
                egui::pos2(rect.min.x + header_w, rect.max.y),
            ],
            egui::Stroke::new(1.0_f32, COLOR_BORDER),
        );
        ui.painter().text(
            header.center(),
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(11.0),
            COLOR_TEXT,
        );
    }

    let used_h = (a1_y - top) + TRACK_ROW_HEIGHT + 8.0;
    let _ = ui.allocate_exact_size(egui::vec2(width, used_h), egui::Sense::hover());
}

fn paint_program_monitor(ui: &mut egui::Ui) -> LogicalRect {
    let full = ui.max_rect();

    // Opaque chrome header — outside the video rect.
    let header = egui::Rect::from_min_size(
        full.min,
        egui::vec2(full.width(), PROGRAM_MONITOR_HEADER_HEIGHT),
    );
    ui.painter()
        .rect_filled(header, egui::CornerRadius::ZERO, COLOR_HEADER);
    ui.painter().line_segment(
        [
            egui::pos2(header.min.x, header.max.y - 0.5),
            egui::pos2(header.max.x, header.max.y - 0.5),
        ],
        egui::Stroke::new(1.0_f32, COLOR_BORDER),
    );
    ui.painter().text(
        egui::pos2(header.min.x + 10.0, header.center().y),
        egui::Align2::LEFT_CENTER,
        "PROGRAM MONITOR",
        egui::FontId::proportional(11.0),
        COLOR_TEXT_MUTED,
    );

    let inset = 1.0_f32;
    let video = egui::Rect::from_min_max(
        egui::pos2(full.min.x + inset, header.max.y + inset),
        egui::pos2(full.max.x - inset, full.max.y - inset),
    );

    // Fill chrome around the video hole; keep the video interior transparent for NV12.
    if video.width() > 0.0 && video.height() > 0.0 {
        let left = egui::Rect::from_min_max(
            egui::pos2(full.min.x, header.max.y),
            egui::pos2(video.min.x, full.max.y),
        );
        let right = egui::Rect::from_min_max(
            egui::pos2(video.max.x, header.max.y),
            egui::pos2(full.max.x, full.max.y),
        );
        let bottom = egui::Rect::from_min_max(
            egui::pos2(video.min.x, video.max.y),
            egui::pos2(video.max.x, full.max.y),
        );
        let top_gap = egui::Rect::from_min_max(
            egui::pos2(video.min.x, header.max.y),
            egui::pos2(video.max.x, video.min.y),
        );
        for band in [left, right, bottom, top_gap] {
            if band.width() > 0.0 && band.height() > 0.0 {
                ui.painter()
                    .rect_filled(band, egui::CornerRadius::ZERO, COLOR_BG);
            }
        }
        ui.painter().rect_stroke(
            video,
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.0_f32, COLOR_BORDER),
            egui::StrokeKind::Inside,
        );
    } else {
        let rest = egui::Rect::from_min_max(egui::pos2(full.min.x, header.max.y), full.max);
        ui.painter()
            .rect_filled(rest, egui::CornerRadius::ZERO, COLOR_MONITOR);
    }

    // Consume the panel so egui does not leave empty interaction gaps.
    let _ = ui.allocate_rect(full, egui::Sense::hover());

    if video.width() <= 0.0 || video.height() <= 0.0 {
        return LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }

    LogicalRect {
        x: video.min.x,
        y: video.min.y,
        width: video.width(),
        height: video.height(),
    }
}

/// Converts a logical egui rect to a physical viewport clamped to the surface.
///
/// Zero or invalid inputs yield a zero-sized viewport without panicking.
pub fn logical_rect_to_physical_viewport(
    rect: LogicalRect,
    pixels_per_point: f32,
    surface_width: u32,
    surface_height: u32,
) -> PhysicalViewport {
    if !(pixels_per_point.is_finite() && pixels_per_point > 0.0)
        || surface_width == 0
        || surface_height == 0
        || !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return PhysicalViewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }

    let mut x0 = (rect.x * pixels_per_point).floor() as i64;
    let mut y0 = (rect.y * pixels_per_point).floor() as i64;
    let mut x1 = ((rect.x + rect.width) * pixels_per_point).ceil() as i64;
    let mut y1 = ((rect.y + rect.height) * pixels_per_point).ceil() as i64;

    let sw = surface_width as i64;
    let sh = surface_height as i64;
    x0 = x0.clamp(0, sw);
    y0 = y0.clamp(0, sh);
    x1 = x1.clamp(0, sw);
    y1 = y1.clamp(0, sh);

    if x1 <= x0 || y1 <= y0 {
        return PhysicalViewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }

    PhysicalViewport {
        x: x0 as u32,
        y: y0 as u32,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    }
}

/// Paints the professional editor shell and returns the video rect plus optional UI action.
///
/// Program Monitor chrome header is outside [`EditorShellOutput::program_monitor`].
/// The video area uses a transparent interior so NV12 remains visible under `LoadOp::Load`.
pub fn paint_editor_shell(ctx: &egui::Context) -> EditorShellOutput {
    let mut action = None;

    egui::TopBottomPanel::top("dvs_ui_top_bar")
        .exact_height(TOP_BAR_HEIGHT)
        .resizable(false)
        .frame(top_bar_frame())
        .show(ctx, |ui| {
            action = paint_top_bar(ui);
        });

    egui::TopBottomPanel::bottom("dvs_ui_timeline")
        .exact_height(TIMELINE_HEIGHT)
        .resizable(false)
        .frame(panel_frame())
        .show(ctx, |ui| {
            paint_timeline(ui);
        });

    egui::SidePanel::left("dvs_ui_media_pool")
        .exact_width(MEDIA_POOL_WIDTH)
        .resizable(false)
        .frame(panel_frame())
        .show(ctx, |ui| {
            paint_media_pool(ui);
        });

    egui::SidePanel::right("dvs_ui_inspector")
        .exact_width(INSPECTOR_WIDTH)
        .resizable(false)
        .frame(panel_frame())
        .show(ctx, |ui| {
            paint_inspector(ui);
        });

    let mut monitor = LogicalRect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            monitor = paint_program_monitor(ui);
        });

    EditorShellOutput {
        program_monitor: monitor,
        action,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_shell(w: f32, h: f32) -> EditorShellOutput {
        let ctx = egui::Context::default();
        apply_editor_theme(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(w, h),
            )),
            ..Default::default()
        };
        let mut output = EditorShellOutput {
            program_monitor: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            action: None,
        };
        let _ = ctx.run(raw, |ctx| {
            output = paint_editor_shell(ctx);
        });
        output
    }

    #[test]
    fn apply_editor_theme_installs_without_panic() {
        let ctx = egui::Context::default();
        apply_editor_theme(&ctx);
        apply_editor_theme(&ctx);
        let visuals = ctx.style().visuals.clone();
        assert!(visuals.dark_mode);
        assert_eq!(visuals.panel_fill, COLOR_PANEL);
        assert_eq!(visuals.window_fill, COLOR_BG);
    }

    #[test]
    fn play_button_produces_start_playback() {
        assert_eq!(play_button_action(true), Some(EditorAction::StartPlayback));
        assert_eq!(play_button_action(false), None);
    }

    #[test]
    fn editor_action_is_consumed_exactly_once() {
        let mut pending = Some(EditorAction::StartPlayback);
        assert_eq!(
            take_editor_action(&mut pending),
            Some(EditorAction::StartPlayback)
        );
        assert_eq!(take_editor_action(&mut pending), None);
        assert_eq!(take_editor_action(&mut pending), None);
    }

    #[test]
    fn logical_to_physical_clamps_to_surface() {
        let rect = LogicalRect {
            x: -10.0,
            y: -5.0,
            width: 2000.0,
            height: 2000.0,
        };
        let vp = logical_rect_to_physical_viewport(rect, 1.0, 1280, 720);
        assert_eq!(vp.x, 0);
        assert_eq!(vp.y, 0);
        assert_eq!(vp.width, 1280);
        assert_eq!(vp.height, 720);
    }

    #[test]
    fn logical_to_physical_zero_surface_is_empty() {
        let rect = LogicalRect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 80.0,
        };
        let vp = logical_rect_to_physical_viewport(rect, 1.0, 0, 720);
        assert_eq!(vp.width, 0);
        assert_eq!(vp.height, 0);
    }

    #[test]
    fn logical_to_physical_scales_with_pixels_per_point() {
        let rect = LogicalRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let vp = logical_rect_to_physical_viewport(rect, 2.0, 800, 600);
        assert_eq!(vp.x, 20);
        assert_eq!(vp.y, 40);
        assert_eq!(vp.width, 200);
        assert_eq!(vp.height, 100);
    }

    #[test]
    fn paint_editor_shell_produces_valid_monitor_below_header() {
        let output = run_shell(1280.0, 720.0);
        assert!(output.action.is_none());
        let monitor = output.program_monitor;
        assert!(monitor.width > 0.0);
        assert!(monitor.height > 0.0);
        assert!(monitor.x >= MEDIA_POOL_WIDTH - 1.0);
        // Video rect must sit below the top bar and the Program Monitor header.
        assert!(monitor.y >= TOP_BAR_HEIGHT + PROGRAM_MONITOR_HEADER_HEIGHT - 1.0);

        let vp = logical_rect_to_physical_viewport(monitor, 1.0, 1280, 720);
        assert!(vp.width > 0);
        assert!(vp.height > 0);
        assert!(vp.x + vp.width <= 1280);
        assert!(vp.y + vp.height <= 720);
    }

    #[test]
    fn program_monitor_header_is_outside_video_rect() {
        let output = run_shell(1280.0, 720.0);
        let monitor = output.program_monitor;
        // Header occupies [TOP_BAR, TOP_BAR + HEADER); video starts at/after that.
        assert!(monitor.y >= TOP_BAR_HEIGHT + PROGRAM_MONITOR_HEADER_HEIGHT - 2.0);
        assert!(monitor.y > TOP_BAR_HEIGHT + 1.0);
    }

    #[test]
    fn placeholders_do_not_emit_editor_action() {
        let output = run_shell(1280.0, 720.0);
        assert_eq!(output.action, None);

        // Pointer over Media Pool / Inspector / Timeline must not invent StartPlayback.
        let ctx = egui::Context::default();
        apply_editor_theme(&ctx);
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 720.0));
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                let _ = paint_editor_shell(ctx);
            },
        );
        let clicks = [
            egui::pos2(80.0, 120.0),   // media pool
            egui::pos2(1180.0, 200.0), // inspector
            egui::pos2(400.0, 650.0),  // timeline
        ];
        for pos in clicks {
            let press = egui::RawInput {
                screen_rect: Some(screen),
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run(press, |ctx| {
                let _ = paint_editor_shell(ctx);
            });
            let release = egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
                ..Default::default()
            };
            let mut action = Some(EditorAction::StartPlayback);
            let _ = ctx.run(release, |ctx| {
                action = paint_editor_shell(ctx).action;
            });
            assert_eq!(action, None, "placeholder click at {pos:?} emitted action");
        }
    }

    #[test]
    fn play_button_click_emits_start_playback_action() {
        let ctx = egui::Context::default();
        apply_editor_theme(&ctx);
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 720.0));

        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                let _ = paint_editor_shell(ctx);
            },
        );

        // Top bar: brand + EDIT badge + Play — click around the Play control.
        let click_pos = egui::pos2(250.0, 16.0);
        let press = egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(press, |ctx| {
            let _ = paint_editor_shell(ctx);
        });

        let release = egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::PointerButton {
                pos: click_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
            ..Default::default()
        };
        let mut action = None;
        let _ = ctx.run(release, |ctx| {
            action = paint_editor_shell(ctx).action;
        });
        assert_eq!(action, Some(EditorAction::StartPlayback));
    }

    #[test]
    fn monitor_rect_remains_valid_for_tall_wide_and_small_windows() {
        for (w, h) in [
            (400.0_f32, 900.0),
            (1600.0, 500.0),
            (1280.0, 720.0),
            (900.0, 600.0),
        ] {
            let monitor = run_shell(w, h).program_monitor;
            let vp = logical_rect_to_physical_viewport(monitor, 1.0, w as u32, h as u32);
            assert!(vp.x + vp.width <= w as u32);
            assert!(vp.y + vp.height <= h as u32);
        }
    }
}
