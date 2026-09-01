//! Bounded playback metrics summary formatting for diagnostics.

use dvs_playback::PlaybackMetrics;

/// Formats a playback metrics summary without per-frame allocation growth.
pub fn format_metrics_summary(metrics: &PlaybackMetrics, time_base: Option<&str>) -> String {
    let mut lines = Vec::with_capacity(16);
    lines.push("=== Do Vale Studio 4 playback summary ===".to_string());
    if let Some(tb) = time_base {
        lines.push(format!("fixture_time_base: {tb}"));
    }
    if let Some(duration) = metrics.playback_media_duration_us() {
        lines.push(format!("expected_media_duration_us: {}", duration.0));
    }
    if let Some(wall) = metrics.monotonic_wall_duration_us() {
        lines.push(format!("measured_monotonic_duration_us: {}", wall.0));
    }
    lines.push(format!("decoded: {}", metrics.frames_decoded()));
    lines.push(format!("presented: {}", metrics.frames_presented()));
    lines.push(format!("late_drops: {}", metrics.frames_dropped_late()));
    lines.push(format!("early_waits: {}", metrics.early_wait_count()));
    lines.push(format!("max_lateness_us: {}", metrics.max_lateness_us()));
    lines.push(format!(
        "average_lateness_us: {}",
        metrics.average_lateness_us()
    ));
    if let (Some(first), Some(last)) = (
        metrics.first_presented_frame_id(),
        metrics.last_presented_frame_id(),
    ) {
        lines.push(format!(
            "frame_id_range: {}..={}",
            first.value(),
            last.value()
        ));
    }
    lines.push(format!("eof_reached: {}", metrics.eof_reached()));
    lines.push(format!(
        "surface_reconfigurations: {}",
        metrics.surface_reconfigurations()
    ));
    lines.join("\n")
}

/// Returns whether a prepared bridge frame should be discarded during shutdown.
pub const fn should_discard_prepared_on_shutdown(has_prepared: bool) -> bool {
    has_prepared
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_summary_has_bounded_line_count() {
        let metrics = dvs_playback::PlaybackMetrics::new();
        let summary = format_metrics_summary(&metrics, Some("1/90000"));
        let line_count = summary.lines().count();
        assert!(line_count <= 16);
        assert!(summary.contains("decoded: 0"));
    }
}
