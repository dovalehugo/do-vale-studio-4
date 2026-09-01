//! FFmpeg `AVD3D11VADeviceContext` lock callbacks for external `device_context` use.

#![allow(unsafe_code)]

use std::any::Any;
use std::ffi::c_void;
use std::rc::Rc;

/// Structural validation failure for [`D3d11ExternalContextLock`] configuration.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum D3d11ExternalContextLockConfigError {
    /// `acquire` was set without a matching `release`.
    AcquireWithoutRelease,
    /// `release` was set without a matching `acquire`.
    ReleaseWithoutAcquire,
    /// Callbacks require a non-null `lock_ctx`.
    CallbacksRequireNonNullContext,
}

/// Opaque owner that keeps FFmpeg `AVHWDeviceContext` (and its `lock_ctx`) alive.
///
/// Construct this from `dvs-decoder` by retaining the session hardware-device
/// `AVBufferRef` before building [`D3d11ExternalContextLock`].
#[derive(Clone, Debug)]
pub struct D3d11ExternalContextLockKeepalive {
    _inner: Rc<dyn Any>,
}

impl D3d11ExternalContextLockKeepalive {
    /// Wraps an owned Rust value that must outlive all lock callback invocations.
    pub fn new<T: Any + 'static>(owner: T) -> Self {
        Self {
            _inner: Rc::new(owner),
        }
    }
}

/// FFmpeg `lock`/`unlock` callbacks protecting `device_context` and `video_context`.
///
/// Production callers must obtain this from `dvs_decoder::DecoderSession::external_context_lock`.
/// Raw construction is [`Self::new_with_keepalive`], which is `unsafe` and requires the caller
/// to prove callback/context validity.
///
/// Decoder-thread only. Not `Send` or `Sync`.
#[derive(Clone, Debug)]
pub struct D3d11ExternalContextLock {
    acquire: Option<unsafe extern "C" fn(*mut c_void)>,
    release: Option<unsafe extern "C" fn(*mut c_void)>,
    lock_ctx: *mut c_void,
    _keepalive: D3d11ExternalContextLockKeepalive,
}

impl D3d11ExternalContextLock {
    /// Validates callback configuration without invoking callbacks or dereferencing pointers.
    ///
    /// A fully absent callback pair (`acquire` and `release` both `None`) is the supported
    /// no-op configuration and permits a null `lock_ctx`.
    pub fn validate_callback_configuration(
        acquire: Option<unsafe extern "C" fn(*mut c_void)>,
        release: Option<unsafe extern "C" fn(*mut c_void)>,
        lock_ctx: *mut c_void,
    ) -> Result<(), D3d11ExternalContextLockConfigError> {
        match (acquire, release) {
            (Some(_), None) => Err(D3d11ExternalContextLockConfigError::AcquireWithoutRelease),
            (None, Some(_)) => Err(D3d11ExternalContextLockConfigError::ReleaseWithoutAcquire),
            (Some(_), Some(_)) if lock_ctx.is_null() => {
                Err(D3d11ExternalContextLockConfigError::CallbacksRequireNonNullContext)
            }
            (None, None) | (Some(_), Some(_)) => Ok(()),
        }
    }

    /// Creates lock callbacks bound to an owned keepalive.
    ///
    /// # Safety
    ///
    /// The caller must ensure all of the following for the entire lifetime of the returned
    /// token and every clone held by active guards:
    ///
    /// - `acquire` and `release` are a matching pair installed for the same lock object.
    /// - Each callback is valid for every invocation during the token's lifetime.
    /// - `lock_ctx` is valid for both callbacks whenever they are non-null.
    /// - `keepalive` owns or otherwise guarantees the lifetime of the callback state
    ///   referenced by `lock_ctx` (for FFmpeg integration: a retained `AVBufferRef` to the
    ///   `AVHWDeviceContext` that owns `lock_ctx`).
    /// - Callback state remains valid until every token clone and active guard is dropped.
    /// - Callbacks obey the expected FFmpeg locking protocol (recursive acquire/release).
    /// - Callbacks do not unwind across the FFI boundary.
    /// - Acquire and release are only called on the current thread under the documented
    ///   decoder-thread-only contract.
    /// - Every successful acquire is paired with exactly one release.
    ///
    /// When both callbacks are `None`, the token is a supported no-op lock and `lock_ctx`
    /// may be null.
    pub unsafe fn new_with_keepalive(
        acquire: Option<unsafe extern "C" fn(*mut c_void)>,
        release: Option<unsafe extern "C" fn(*mut c_void)>,
        lock_ctx: *mut c_void,
        keepalive: D3d11ExternalContextLockKeepalive,
    ) -> Result<Self, D3d11ExternalContextLockConfigError> {
        Self::validate_callback_configuration(acquire, release, lock_ctx)?;
        Ok(Self {
            acquire,
            release,
            lock_ctx,
            _keepalive: keepalive,
        })
    }

    pub(crate) fn acquire(&self) {
        if let (Some(acquire), ctx) = (self.acquire, self.lock_ctx)
            && !ctx.is_null()
        {
            // SAFETY: Production tokens are built only from audited FFmpeg state; test tokens
            // document matching callback/`lock_ctx` validity at the unsafe constructor site.
            unsafe {
                acquire(ctx);
            }
        }
    }

    pub(crate) fn release(&self) {
        if let (Some(release), ctx) = (self.release, self.lock_ctx)
            && !ctx.is_null()
        {
            // SAFETY: `release` is only called from RAII guards after a successful `acquire`.
            unsafe {
                release(ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct TestLockState {
        depth: Cell<u32>,
        lock_calls: Cell<u32>,
        unlock_calls: Cell<u32>,
    }

    extern "C" fn test_lock(ctx: *mut c_void) {
        // SAFETY: Test-only callback; `ctx` points at `TestLockState`.
        let state = unsafe { &*(ctx as *mut TestLockState) };
        state.depth.set(state.depth.get().saturating_add(1));
        state
            .lock_calls
            .set(state.lock_calls.get().saturating_add(1));
    }

    extern "C" fn test_unlock(ctx: *mut c_void) {
        // SAFETY: Test-only callback; `ctx` points at `TestLockState`.
        let state = unsafe { &*(ctx as *mut TestLockState) };
        state.depth.set(state.depth.get().saturating_sub(1));
        state
            .unlock_calls
            .set(state.unlock_calls.get().saturating_add(1));
    }

    fn test_lock_bundle() -> (D3d11ExternalContextLock, Rc<TestLockState>) {
        let state = Rc::new(TestLockState {
            depth: Cell::new(0),
            lock_calls: Cell::new(0),
            unlock_calls: Cell::new(0),
        });
        let keepalive = D3d11ExternalContextLockKeepalive::new(state.clone());
        // SAFETY: `test_lock`/`test_unlock` are a matching pair; `lock_ctx` points at `state`
        // kept alive by `keepalive`; callbacks are decoder-thread test stubs that do not unwind.
        let lock = unsafe {
            D3d11ExternalContextLock::new_with_keepalive(
                Some(test_lock),
                Some(test_unlock),
                Rc::as_ptr(&state) as *mut c_void,
                keepalive,
            )
        }
        .expect("valid test lock configuration");
        (lock, state)
    }

    struct TestGuard {
        lock: D3d11ExternalContextLock,
        acquired: bool,
    }

    impl TestGuard {
        fn acquire(lock: D3d11ExternalContextLock) -> Self {
            lock.acquire();
            Self {
                lock,
                acquired: true,
            }
        }
    }

    impl Drop for TestGuard {
        fn drop(&mut self) {
            if self.acquired {
                self.lock.release();
            }
        }
    }

    #[test]
    fn validate_rejects_acquire_without_release() {
        let err = D3d11ExternalContextLock::validate_callback_configuration(
            Some(test_lock),
            None,
            std::ptr::null_mut(),
        )
        .expect_err("acquire without release");
        assert_eq!(
            err,
            D3d11ExternalContextLockConfigError::AcquireWithoutRelease
        );
    }

    #[test]
    fn validate_rejects_release_without_acquire() {
        let err = D3d11ExternalContextLock::validate_callback_configuration(
            None,
            Some(test_unlock),
            std::ptr::null_mut(),
        )
        .expect_err("release without acquire");
        assert_eq!(
            err,
            D3d11ExternalContextLockConfigError::ReleaseWithoutAcquire
        );
    }

    #[test]
    fn validate_rejects_callbacks_with_null_context() {
        let err = D3d11ExternalContextLock::validate_callback_configuration(
            Some(test_lock),
            Some(test_unlock),
            std::ptr::null_mut(),
        )
        .expect_err("callbacks with null context");
        assert_eq!(
            err,
            D3d11ExternalContextLockConfigError::CallbacksRequireNonNullContext
        );
    }

    #[test]
    fn validate_accepts_no_op_configuration() {
        D3d11ExternalContextLock::validate_callback_configuration(None, None, std::ptr::null_mut())
            .expect("no-op lock");
    }

    #[test]
    fn guard_unlocks_on_success() {
        let (lock, state) = test_lock_bundle();
        {
            let _guard = TestGuard::acquire(lock);
            assert_eq!(state.lock_calls.get(), 1);
            assert_eq!(state.depth.get(), 1);
        }
        assert_eq!(state.unlock_calls.get(), 1);
        assert_eq!(state.depth.get(), 0);
    }

    #[test]
    fn guard_unlocks_on_early_return() {
        let (lock, state) = test_lock_bundle();
        let result: Result<(), ()> = {
            let _guard = TestGuard::acquire(lock);
            assert_eq!(state.lock_calls.get(), 1);
            Err(())
        };
        assert!(result.is_err());
        assert_eq!(state.unlock_calls.get(), 1);
        assert_eq!(state.depth.get(), 0);
    }

    #[test]
    fn guard_unlocks_on_unwind() {
        let (lock, state) = test_lock_bundle();
        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = TestGuard::acquire(lock);
            panic!("after lock");
        }));
        assert!(panic_result.is_err());
        assert_eq!(state.unlock_calls.get(), 1);
        assert_eq!(state.depth.get(), 0);
    }

    #[test]
    fn recursive_lock_behavior_preserved() {
        let (lock, state) = test_lock_bundle();
        let _outer = TestGuard::acquire(lock.clone());
        let _inner = TestGuard::acquire(lock);
        assert_eq!(state.depth.get(), 2);
        drop(_inner);
        assert_eq!(state.depth.get(), 1);
        assert_eq!(state.unlock_calls.get(), 1);
        drop(_outer);
        assert_eq!(state.depth.get(), 0);
        assert_eq!(state.unlock_calls.get(), 2);
    }

    #[test]
    fn keepalive_retains_owner_after_outer_drop() {
        let (lock, state) = test_lock_bundle();
        drop(state);
        lock.acquire();
        lock.release();
    }

    #[test]
    fn null_no_op_configuration_skips_callbacks() {
        let keepalive = D3d11ExternalContextLockKeepalive::new(());
        // SAFETY: Both callbacks are absent; null `lock_ctx` is the documented no-op case.
        let lock = unsafe {
            D3d11ExternalContextLock::new_with_keepalive(
                None,
                None,
                std::ptr::null_mut(),
                keepalive,
            )
        }
        .expect("no-op lock");
        lock.acquire();
        lock.release();
    }
}
