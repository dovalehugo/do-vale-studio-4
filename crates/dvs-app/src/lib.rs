#![forbid(unsafe_code)]

//! Production composition root for Do Vale Studio 4.

mod config;
mod display;
mod error;
mod metrics_summary;
mod runtime;
mod shutdown;
mod state;

#[cfg(target_os = "windows")]
mod egui_overlay;
#[cfg(target_os = "windows")]
mod window_app;
#[cfg(target_os = "windows")]
mod windows;

pub use config::{AppConfig, RunMode, event_loop_allows_any_thread, parse_args};
pub use display::{can_render_display_frame, redraw_preserves_state};
pub use error::AppError;
pub use metrics_summary::{format_metrics_summary, should_discard_prepared_on_shutdown};
pub use runtime::run;
pub use state::AppState;
