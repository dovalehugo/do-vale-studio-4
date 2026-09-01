//! Application runtime entry points.

use crate::config::AppConfig;
use crate::error::AppError;

/// Runs the production application using the validated configuration.
pub fn run(config: AppConfig) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        crate::window_app::run_windows_app(config)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = config;
        Err(AppError::UnsupportedPlatform)
    }
}
