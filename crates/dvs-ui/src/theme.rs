//! Professional dark editor theme for Do Vale Studio 4.

use egui::{Color32, Context, CornerRadius, Stroke, Visuals};

/// Deep charcoal application background.
pub const BG_APP: Color32 = Color32::from_rgb(24, 26, 30);
/// Slightly lighter panel surface.
pub const BG_PANEL: Color32 = Color32::from_rgb(32, 35, 40);
/// Program monitor canvas background (GPU-composited; not painted by egui).
#[allow(dead_code)]
pub const BG_MONITOR: Color32 = Color32::from_rgb(8, 8, 10);
/// Subtle panel border.
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(52, 56, 64);
/// Restrained blue-violet accent.
pub const ACCENT: Color32 = Color32::from_rgb(88, 128, 220);
/// Accent hover state.
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(108, 148, 238);
/// Accent active/pressed state.
pub const ACCENT_ACTIVE: Color32 = Color32::from_rgb(68, 108, 196);
/// Primary text.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(220, 224, 232);
/// Secondary/muted text.
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(148, 154, 166);
/// Disabled text and controls.
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(96, 100, 110);

/// Applies the production editor visual foundation.
pub fn apply_editor_theme(context: &Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_APP;
    visuals.extreme_bg_color = BG_APP;
    visuals.faint_bg_color = Color32::from_rgb(40, 43, 48);
    visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_SECONDARY);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(44, 48, 54);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(54, 58, 66);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.active.bg_fill = ACCENT_ACTIVE;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.open.bg_fill = ACCENT;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = Color32::from_rgb(220, 180, 80);
    visuals.error_fg_color = Color32::from_rgb(220, 96, 96);
    visuals.window_corner_radius = CornerRadius::same(4);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    context.set_visuals(visuals);

    let mut style = (*context.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(8);
    style.spacing.indent = 16.0;
    context.set_style(style);
}
