//! View-model construction from application state.

use dvs_playback::PlaybackMetrics;
use dvs_ui::{EditorPlaybackState, EditorViewModel};

use crate::config::AppConfig;
use crate::state::AppState;
use crate::windows::video_pipeline::VideoPipeline;

fn map_playback_state(state: AppState) -> EditorPlaybackState {
    match state {
        AppState::Initializing => EditorPlaybackState::Initializing,
        AppState::Ready => EditorPlaybackState::Ready,
        AppState::Playing => EditorPlaybackState::Playing,
        AppState::Ended => EditorPlaybackState::Ended,
        AppState::Closing | AppState::Fatal => EditorPlaybackState::Ended,
    }
}

/// Builds the editor view-model from current application state.
pub fn build_editor_view_model(
    config: &AppConfig,
    state: AppState,
    pipeline: Option<&VideoPipeline>,
) -> EditorViewModel {
    let media_name = config
        .input()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("video")
        .to_string();

    let (current_time_us, duration_us, frame_id, status_text) = if let Some(pipeline) = pipeline {
        let metrics = pipeline.metrics();
        let current = current_time_from_metrics(metrics, pipeline);
        let duration = metrics.playback_media_duration_us().map(|value| value.0);
        let frame_id = metrics.last_presented_frame_id().map(|id| id.value());
        let status = status_text_for_state(state, pipeline);
        (current, duration, frame_id, status)
    } else {
        (None, None, None, "Starting…".to_string())
    };

    EditorViewModel {
        app_title: "Do Vale Studio 4".to_string(),
        media_name,
        playback_state: map_playback_state(state),
        current_time_us,
        duration_us,
        frame_id,
        status_text,
    }
}

fn current_time_from_metrics(metrics: &PlaybackMetrics, pipeline: &VideoPipeline) -> Option<i64> {
    if let Some(last) = pipeline.last_pts() {
        return Some(last.pts());
    }
    if let Some(first) = pipeline.first_pts() {
        return Some(first.pts());
    }
    metrics.playback_media_duration_us().map(|value| value.0)
}

fn status_text_for_state(state: AppState, pipeline: &VideoPipeline) -> String {
    match state {
        AppState::Initializing => "Initializing".to_string(),
        AppState::Ready => {
            if pipeline.playback_started() {
                "Ready".to_string()
            } else {
                "Press Play or SPACE to start".to_string()
            }
        }
        AppState::Playing => "Playing".to_string(),
        AppState::Ended => "Ended".to_string(),
        AppState::Closing => "Closing".to_string(),
        AppState::Fatal => "Error".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("dvs-app-vm-{name}-{nanos}.mp4"))
    }

    #[test]
    fn view_model_from_ready_state_without_pipeline() {
        let path = temp_file("ready");
        fs::write(&path, b"test").expect("write");
        let config = crate::config::AppConfig::interactive(&path).expect("config");
        let model = build_editor_view_model(&config, AppState::Ready, None);
        assert_eq!(model.playback_state, EditorPlaybackState::Ready);
        assert_eq!(
            model.media_name,
            path.file_name().unwrap().to_str().unwrap()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn ended_maps_to_editor_ended() {
        let path = temp_file("ended");
        fs::write(&path, b"test").expect("write");
        let config = crate::config::AppConfig::interactive(&path).expect("config");
        let model = build_editor_view_model(&config, AppState::Ended, None);
        assert_eq!(model.playback_state, EditorPlaybackState::Ended);
        let _ = fs::remove_file(path);
    }
}
