//! Main editor shell UI.

use egui::{
    Align, Color32, Context, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, UiBuilder, Vec2,
};

use crate::intent::{EditorUiOutput, UiIntent};
use crate::layout::{
    EditorLayout, MONITOR_INNER_PADDING, compute_editor_layout, program_monitor_video_rect,
};
use crate::model::EditorViewModel;
use crate::theme::{
    ACCENT, ACCENT_ACTIVE, ACCENT_HOVER, BG_PANEL, BORDER_SUBTLE, TEXT_DISABLED, TEXT_PRIMARY,
    TEXT_SECONDARY,
};
use crate::timecode::format_timecode_range;

/// Persistent editor UI state.
pub struct EditorUi {
    layout: Option<EditorLayout>,
    #[cfg(test)]
    last_play_button_rect: Option<egui::Rect>,
}

impl EditorUi {
    /// Creates the editor shell and applies the production theme once.
    pub fn new(context: &Context) -> Self {
        crate::theme::apply_editor_theme(context);
        Self {
            layout: None,
            #[cfg(test)]
            last_play_button_rect: None,
        }
    }

    /// Lays out the editor shell and returns monitor geometry plus intents.
    pub fn show(&mut self, context: &Context, model: &EditorViewModel) -> EditorUiOutput {
        let screen = context.available_rect();
        let layout = compute_editor_layout(screen.size());
        self.layout = Some(layout);

        let mut output = EditorUiOutput::empty();
        output.program_monitor_rect = program_monitor_video_rect(layout);

        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show(context, |ui| {
                ui.set_min_size(screen.size());
                paint_top_bar(ui, layout.top_bar, model);
                paint_media_panel(ui, layout.media_panel, model);
                paint_inspector_panel(ui, layout.inspector_panel);
                paint_program_monitor_chrome(ui, layout.program_monitor, model);
                let mut play_button_rect = None;
                if let Some(intent) =
                    paint_transport(ui, layout.transport, model, &mut play_button_rect)
                {
                    output.intents.push(intent);
                }
                #[cfg(test)]
                {
                    self.last_play_button_rect = play_button_rect;
                }
                paint_timeline_placeholder(ui, layout.timeline);
            });

        output
    }

    /// Returns the last computed layout when available.
    pub const fn last_layout(&self) -> Option<EditorLayout> {
        self.layout
    }

    /// Returns the last laid-out Play button rectangle (test-only).
    #[cfg(test)]
    pub const fn last_play_button_rect(&self) -> Option<egui::Rect> {
        self.last_play_button_rect
    }
}

fn paint_top_bar(ui: &mut Ui, rect: egui::Rect, model: &EditorViewModel) {
    let _ = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        Frame::new()
            .fill(BG_PANEL)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&model.app_title)
                            .strong()
                            .color(TEXT_PRIMARY)
                            .size(15.0),
                    );
                    ui.separator();
                    ui.label(RichText::new(&model.media_name).color(TEXT_SECONDARY));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(false, |ui| {
                            let _ = ui.button("Workspace");
                            let _ = ui.button("Export");
                        });
                    });
                });
            });
    });
}

fn paint_media_panel(ui: &mut Ui, rect: egui::Rect, model: &EditorViewModel) {
    let _ = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        panel_frame(ui, "Media", |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new("No import yet").strong().color(TEXT_PRIMARY));
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Media import and browsing are not available in this milestone.")
                        .color(TEXT_SECONDARY)
                        .size(12.0),
                );
                ui.add_space(16.0);
                ui.label(
                    RichText::new(format!("Current input: {}", model.media_name))
                        .color(TEXT_SECONDARY)
                        .size(12.0),
                );
            });
        });
    });
}

fn paint_inspector_panel(ui: &mut Ui, rect: egui::Rect) {
    let _ = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        panel_frame(ui, "Inspector", |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(
                    RichText::new("No clip selected")
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Inspector controls and effects will appear here in a future milestone.",
                    )
                    .color(TEXT_SECONDARY)
                    .size(12.0),
                );
            });
        });
    });
}

fn paint_program_monitor_chrome(ui: &mut Ui, rect: egui::Rect, model: &EditorViewModel) {
    let _ = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        let inner = rect.shrink2(Vec2::splat(MONITOR_INNER_PADDING));
        let response = ui.allocate_rect(rect, Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, BORDER_SUBTLE),
            egui::StrokeKind::Inside,
        );

        let label_pos = rect.left_top() + Vec2::new(10.0, 8.0);
        painter.text(
            label_pos,
            egui::Align2::LEFT_TOP,
            "Program Monitor",
            egui::FontId::proportional(12.0),
            TEXT_SECONDARY,
        );

        if let Some(frame_id) = model.frame_id {
            let status = format!("Frame {}", frame_id);
            painter.text(
                rect.right_top() + Vec2::new(-10.0, 8.0),
                egui::Align2::RIGHT_TOP,
                status,
                egui::FontId::proportional(11.0),
                TEXT_SECONDARY,
            );
        }

        let _ = (inner, response);
    });
}

fn paint_transport(
    ui: &mut Ui,
    rect: egui::Rect,
    model: &EditorViewModel,
    play_button_rect: &mut Option<egui::Rect>,
) -> Option<UiIntent> {
    let mut intent = None;
    let _ = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        Frame::new()
            .fill(BG_PANEL)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let play_enabled = model.playback_state.play_enabled();
                    let play_button = play_button(ui, play_enabled);
                    *play_button_rect = Some(play_button.rect);
                    if play_button.clicked() && play_enabled {
                        intent = Some(UiIntent::StartPlayback);
                    }

                    ui.separator();
                    ui.label(
                        RichText::new(format_timecode_range(
                            model.current_time_us,
                            model.duration_us,
                        ))
                        .monospace()
                        .color(TEXT_PRIMARY),
                    );
                    ui.separator();
                    ui.label(RichText::new(model.playback_state_label()).color(TEXT_SECONDARY));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&model.status_text).color(TEXT_SECONDARY));
                    });
                });
            });
    });
    intent
}

fn paint_timeline_placeholder(ui: &mut Ui, rect: egui::Rect) {
    let _ = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        panel_frame(ui, "Timeline", |ui| {
            let ruler = ui.available_rect_before_wrap().shrink2(Vec2::new(0.0, 8.0));
            let painter = ui.painter_at(ruler);
            painter.rect_filled(
                egui::Rect::from_min_size(ruler.left_top(), Vec2::new(ruler.width(), 22.0)),
                2.0,
                Color32::from_rgb(40, 43, 48),
            );
            painter.line_segment(
                [
                    ruler.left_bottom() + Vec2::new(0.0, -ruler.height() + 30.0),
                    ruler.right_bottom() + Vec2::new(0.0, -ruler.height() + 30.0),
                ],
                Stroke::new(1.0_f32, BORDER_SUBTLE),
            );
            ui.add_space(30.0);
            ui.label(
                RichText::new("Timeline editing is upcoming — no clips, playhead seeking, or track editing yet.")
                    .color(TEXT_SECONDARY)
                    .size(12.0),
            );
        });
    });
}

fn panel_frame(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(BG_PANEL)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.label(RichText::new(title).strong().color(TEXT_PRIMARY));
            ui.separator();
            add_contents(ui);
        });
}

fn play_button(ui: &mut Ui, enabled: bool) -> egui::Response {
    let size = Vec2::new(34.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let fill = if !enabled {
            Color32::from_rgb(50, 54, 60)
        } else if response.is_pointer_button_down_on() {
            ACCENT_ACTIVE
        } else if response.hovered() {
            ACCENT_HOVER
        } else {
            ACCENT
        };
        painter.rect_filled(rect, 4.0, fill);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, BORDER_SUBTLE),
            egui::StrokeKind::Inside,
        );

        let triangle = [
            rect.center() + Vec2::new(-4.0, -7.0),
            rect.center() + Vec2::new(-4.0, 7.0),
            rect.center() + Vec2::new(8.0, 0.0),
        ];
        let triangle_color = if enabled { TEXT_PRIMARY } else { TEXT_DISABLED };
        painter.add(egui::Shape::convex_polygon(
            triangle.to_vec(),
            triangle_color,
            Stroke::NONE,
        ));
    }
    if !enabled {
        return response;
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::EditorUiOutput;
    use crate::layout::{compute_editor_layout, program_monitor_video_rect};
    use crate::model::EditorPlaybackState;

    fn sample_model(state: EditorPlaybackState) -> EditorViewModel {
        EditorViewModel {
            app_title: "Do Vale Studio 4".to_string(),
            media_name: "clip.mp4".to_string(),
            playback_state: state,
            current_time_us: Some(1_000_000),
            duration_us: Some(19_350_000),
            frame_id: Some(42),
            status_text: "Ready".to_string(),
        }
    }

    #[test]
    fn play_enabled_only_in_ready() {
        assert!(EditorPlaybackState::Ready.play_enabled());
        assert!(!EditorPlaybackState::Playing.play_enabled());
        assert!(!EditorPlaybackState::Ended.play_enabled());
    }

    #[test]
    fn play_disabled_while_playing_or_ended() {
        assert!(EditorPlaybackState::Playing.play_disabled());
        assert!(EditorPlaybackState::Ended.play_disabled());
        assert!(!EditorPlaybackState::Ready.play_disabled());
    }

    #[test]
    fn view_model_state_labels() {
        let ready = sample_model(EditorPlaybackState::Ready);
        assert_eq!(ready.playback_state_label(), "Ready");
        let playing = sample_model(EditorPlaybackState::Playing);
        assert_eq!(playing.playback_state_label(), "Playing");
    }

    #[test]
    fn program_monitor_video_rect_is_inside_chrome() {
        let layout = compute_editor_layout(Vec2::new(1280.0, 800.0));
        let video = program_monitor_video_rect(layout);
        assert!(layout.program_monitor.contains_rect(video));
        assert!(video.width() > 0.0);
        assert!(video.height() > 0.0);
    }

    fn show_editor_after_click(
        editor: &mut EditorUi,
        context: &egui::Context,
        model: &EditorViewModel,
        screen_rect: egui::Rect,
        click_pos: egui::Pos2,
    ) -> EditorUiOutput {
        let mut output = EditorUiOutput::empty();
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                time: Some(1.0),
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
            },
            |ctx| {
                let _ = editor.show(ctx, model);
            },
        );
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                time: Some(1.1),
                events: vec![
                    egui::Event::PointerMoved(click_pos),
                    egui::Event::PointerButton {
                        pos: click_pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                output = editor.show(ctx, model);
            },
        );
        output
    }

    #[test]
    fn play_pointer_click_emits_start_playback() {
        let context = egui::Context::default();
        let model = sample_model(EditorPlaybackState::Ready);
        let screen = Vec2::new(1280.0, 800.0);
        let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), screen);
        let transport = compute_editor_layout(screen).transport;
        let mut play_button_rect = None;

        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ctx, |ui| {
                        let _ = paint_transport(ui, transport, &model, &mut play_button_rect);
                    });
            },
        );
        let play_center = play_button_rect.expect("play button rect").center();

        let mut intent = None;
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                time: Some(1.0),
                events: vec![
                    egui::Event::PointerMoved(play_center),
                    egui::Event::PointerButton {
                        pos: play_center,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                    egui::Event::PointerButton {
                        pos: play_center,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                intent = egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ctx, |ui| {
                        paint_transport(ui, transport, &model, &mut play_button_rect)
                    })
                    .inner;
            },
        );

        assert_eq!(intent, Some(UiIntent::StartPlayback));
    }

    #[test]
    fn play_two_frame_pointer_click_emits_start_playback() {
        let context = egui::Context::default();
        let mut editor = EditorUi::new(&context);
        let model = sample_model(EditorPlaybackState::Ready);
        let screen = Vec2::new(1280.0, 800.0);
        let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), screen);

        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                let _ = editor.show(ctx, &model);
            },
        );
        let play_center = editor
            .last_play_button_rect()
            .expect("play button rect")
            .center();

        let output =
            show_editor_after_click(&mut editor, &context, &model, screen_rect, play_center);
        assert_eq!(
            output
                .intents
                .iter()
                .filter(|intent| **intent == UiIntent::StartPlayback)
                .count(),
            1
        );
    }

    #[test]
    fn play_click_emits_no_intent_while_playing() {
        let context = egui::Context::default();
        let mut editor = EditorUi::new(&context);
        let model = sample_model(EditorPlaybackState::Playing);
        let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(1280.0, 800.0));

        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                let _ = editor.show(ctx, &model);
            },
        );
        let play_center = editor
            .last_play_button_rect()
            .expect("play button rect")
            .center();
        let output =
            show_editor_after_click(&mut editor, &context, &model, screen_rect, play_center);
        assert!(!output.intents.contains(&UiIntent::StartPlayback));
    }

    #[test]
    fn play_click_emits_no_intent_when_ended() {
        let context = egui::Context::default();
        let mut editor = EditorUi::new(&context);
        let model = sample_model(EditorPlaybackState::Ended);
        let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(1280.0, 800.0));

        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                let _ = editor.show(ctx, &model);
            },
        );
        let play_center = editor
            .last_play_button_rect()
            .expect("play button rect")
            .center();
        let output =
            show_editor_after_click(&mut editor, &context, &model, screen_rect, play_center);
        assert!(!output.intents.contains(&UiIntent::StartPlayback));
    }

    #[test]
    fn click_outside_play_button_emits_no_start_playback() {
        let context = egui::Context::default();
        let mut editor = EditorUi::new(&context);
        let model = sample_model(EditorPlaybackState::Ready);
        let screen = Vec2::new(1280.0, 800.0);
        let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), screen);
        let layout = compute_editor_layout(screen);

        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                let _ = editor.show(ctx, &model);
            },
        );
        let outside = layout.timeline.center();
        let output = show_editor_after_click(&mut editor, &context, &model, screen_rect, outside);
        assert!(!output.intents.contains(&UiIntent::StartPlayback));
    }
}
