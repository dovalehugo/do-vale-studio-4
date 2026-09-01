//! Display rendering helpers — stateless with respect to application transitions.

use crate::state::AppState;

/// Returns whether a redraw in the given state should render without changing state.
pub const fn redraw_preserves_state(state: AppState) -> bool {
    matches!(state, AppState::Ready | AppState::Playing | AppState::Ended)
}

/// Returns whether the application state permits rendering a held display frame.
pub const fn can_render_display_frame(state: AppState) -> bool {
    matches!(state, AppState::Ready | AppState::Ended)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redraw_preserves_ready_playing_ended() {
        assert!(redraw_preserves_state(AppState::Ready));
        assert!(redraw_preserves_state(AppState::Playing));
        assert!(redraw_preserves_state(AppState::Ended));
        assert!(!redraw_preserves_state(AppState::Initializing));
    }

    #[test]
    fn display_render_allowed_in_ready_and_ended() {
        assert!(can_render_display_frame(AppState::Ready));
        assert!(can_render_display_frame(AppState::Ended));
        assert!(!can_render_display_frame(AppState::Playing));
    }
}
