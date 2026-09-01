//! Explicit GPU resource shutdown helpers.

use crate::error::AppError;
use crate::metrics_summary::should_discard_prepared_on_shutdown;
use crate::windows::video_pipeline::VideoPipeline;

/// Releases prepared bridge resources when a GPU context is still available.
///
/// Idempotent: returns `Ok(())` when there is no prepared frame or no GPU context.
pub fn release_prepared_bridge_frame(
    gpu: Option<&dvs_gpu::GpuContext>,
    pipeline: Option<&mut VideoPipeline>,
) -> Result<(), AppError> {
    let Some(pipeline) = pipeline else {
        return Ok(());
    };
    if !should_discard_prepared_on_shutdown(pipeline.has_prepared_frame()) {
        return Ok(());
    }
    let Some(gpu) = gpu else {
        return Ok(());
    };
    pipeline.release_prepared_on_exit(gpu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use std::sync::Mutex;

    #[test]
    fn prepared_shutdown_decision_requires_prepared_frame() {
        assert!(should_discard_prepared_on_shutdown(true));
        assert!(!should_discard_prepared_on_shutdown(false));
    }

    #[test]
    fn release_without_gpu_is_noop() {
        assert!(release_prepared_bridge_frame(None, None).is_ok());
    }

    #[test]
    fn struct_fields_drop_in_declaration_order() {
        static LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

        struct DropProbe(&'static str);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                LOG.lock().expect("log").push(self.0);
            }
        }

        #[allow(dead_code)]
        struct Probe {
            first: DropProbe,
            second: DropProbe,
        }

        {
            let _probe = Probe {
                first: DropProbe("first"),
                second: DropProbe("second"),
            };
        }

        let log = LOG.lock().expect("log");
        assert_eq!(&*log, &["first", "second"]);
    }

    #[test]
    fn close_from_live_states_enters_closing() {
        for state in [
            AppState::Initializing,
            AppState::Ready,
            AppState::Playing,
            AppState::Ended,
        ] {
            assert_eq!(state.begin_close(), AppState::Closing);
        }
        assert_eq!(AppState::Fatal.begin_close(), AppState::Fatal);
        assert_eq!(AppState::Closing.begin_close(), AppState::Closing);
    }
}
