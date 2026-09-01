//! CLI parsing and application configuration.

use std::path::{Path, PathBuf};

use crate::error::AppError;

const HELP: &str = "\
Do Vale Studio 4 — production video application

Usage:
  dvs-app --input <video-path>

Options:
  --input <path>  Required input video file
  --help          Show this help message
";

/// How the application should behave after startup.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RunMode {
    /// Wait for SPACE before PTS playback and hold the last frame at EOF.
    Interactive,
    /// Start playback immediately; optional post-EOF resize validation before exit.
    SmokeTest {
        /// Resize and redraw twice after EOF before shutting down.
        post_eof_resize: bool,
    },
}

/// Validated production application configuration.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AppConfig {
    input: PathBuf,
    run_mode: RunMode,
}

/// Returns whether the event loop may be created off the main thread.
pub const fn event_loop_allows_any_thread(run_mode: RunMode) -> bool {
    matches!(run_mode, RunMode::SmokeTest { .. })
}

impl AppConfig {
    /// Creates a validated configuration for interactive playback.
    pub fn interactive(input: impl Into<PathBuf>) -> Result<Self, AppError> {
        Self::new(input, RunMode::Interactive)
    }

    /// Creates a validated configuration for automated smoke testing.
    pub fn smoke_test(input: impl Into<PathBuf>) -> Result<Self, AppError> {
        Self::new(
            input,
            RunMode::SmokeTest {
                post_eof_resize: false,
            },
        )
    }

    /// Creates a smoke-test configuration that validates post-EOF resize redraws.
    pub fn smoke_test_with_post_eof_resize(input: impl Into<PathBuf>) -> Result<Self, AppError> {
        Self::new(
            input,
            RunMode::SmokeTest {
                post_eof_resize: true,
            },
        )
    }

    /// Returns whether the smoke test should validate post-EOF resize redraws.
    pub const fn smoke_post_eof_resize(&self) -> bool {
        matches!(
            self.run_mode,
            RunMode::SmokeTest {
                post_eof_resize: true,
            }
        )
    }

    fn new(input: impl Into<PathBuf>, run_mode: RunMode) -> Result<Self, AppError> {
        let input = input.into();
        validate_input_path(&input)?;
        Ok(Self { input, run_mode })
    }

    /// Returns the validated input video path.
    pub fn input(&self) -> &Path {
        &self.input
    }

    /// Returns the configured run mode.
    pub const fn run_mode(&self) -> RunMode {
        self.run_mode
    }

    /// Returns a window title including the application name and input filename.
    pub fn window_title(&self) -> String {
        let file_name = self
            .input
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("video");
        format!("Do Vale Studio 4 — {file_name}")
    }
}

/// Parses CLI arguments into an [`AppConfig`].
pub fn parse_args<I, S>(args: I) -> Result<AppConfig, AppError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = args.into_iter();
    let _program = iter.next();

    let mut input = None;
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        match arg {
            "--help" | "-h" => {
                return Err(AppError::Config(HELP.to_string()));
            }
            "--input" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::Config("missing value for --input".to_string()))?;
                input = Some(PathBuf::from(value.as_ref()));
            }
            other if other.starts_with("--input=") => {
                let value = other
                    .split_once('=')
                    .map(|(_, value)| value)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AppError::Config("missing value for --input".to_string()))?;
                input = Some(PathBuf::from(value));
            }
            unknown => {
                return Err(AppError::Config(format!("unknown argument: {unknown}")));
            }
        }
    }

    let input = input.ok_or_else(|| {
        AppError::Config("missing required argument: --input <video-path>".to_string())
    })?;
    AppConfig::interactive(input)
}

fn validate_input_path(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Err(AppError::InvalidInput {
            path: path.to_path_buf(),
        });
    }
    if !path.is_file() {
        return Err(AppError::InvalidInput {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("dvs-app-{name}-{nanos}.mp4"))
    }

    #[test]
    fn parses_input_argument() {
        let path = temp_file("parse");
        fs::write(&path, b"test").expect("write temp");
        let config =
            parse_args(["dvs-app", "--input", path.to_str().expect("utf8")]).expect("parse");
        assert_eq!(config.input(), path.as_path());
        assert_eq!(config.run_mode(), RunMode::Interactive);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn parses_input_with_spaces() {
        let path = temp_file("path with spaces");
        fs::write(&path, b"test").expect("write temp");
        let config =
            parse_args(["dvs-app", "--input", path.to_str().expect("utf8")]).expect("parse");
        assert_eq!(config.input(), path.as_path());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_input_is_rejected() {
        let err = parse_args(["dvs-app"]).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
    }

    #[test]
    fn unknown_argument_is_rejected() {
        let path = temp_file("unknown");
        fs::write(&path, b"test").expect("write temp");
        let err =
            parse_args(["dvs-app", "--input", path.to_str().expect("utf8"), "--loop"]).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_file_is_rejected() {
        let err = parse_args([
            "dvs-app",
            "--input",
            r"C:\no\such\file\definitely-missing.mp4",
        ])
        .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput { .. }));
    }

    #[test]
    fn smoke_test_uses_any_thread_event_loop() {
        assert!(event_loop_allows_any_thread(RunMode::SmokeTest {
            post_eof_resize: false,
        }));
        assert!(!event_loop_allows_any_thread(RunMode::Interactive));
    }

    #[test]
    fn smoke_test_mode_is_distinct_from_interactive() {
        let path = temp_file("smoke");
        fs::write(&path, b"test").expect("write temp");
        let config = AppConfig::smoke_test(&path).expect("smoke");
        assert_eq!(
            config.run_mode(),
            RunMode::SmokeTest {
                post_eof_resize: false,
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn smoke_test_with_post_eof_resize_flag() {
        let path = temp_file("smoke-resize");
        fs::write(&path, b"test").expect("write temp");
        let config = AppConfig::smoke_test_with_post_eof_resize(&path).expect("smoke");
        assert!(config.smoke_post_eof_resize());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn window_title_includes_application_name() {
        let path = temp_file("title");
        fs::write(&path, b"test").expect("write temp");
        let config = AppConfig::interactive(&path).expect("config");
        assert!(config.window_title().contains("Do Vale Studio 4"));
        let _ = fs::remove_file(path);
    }
}
