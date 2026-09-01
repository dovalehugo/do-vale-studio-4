//! RAII ownership for NT shared kernel handles.

#![allow(unsafe_code)]
#![allow(dead_code)] // `handle()` consumed by Integration 3C

use windows::Win32::Foundation::{CloseHandle, E_HANDLE, HANDLE, INVALID_HANDLE_VALUE};

use crate::error::GpuError;

/// Owns a valid NT shared `HANDLE` and closes it exactly once on drop.
pub(crate) struct OwnedNtHandle(HANDLE);

impl OwnedNtHandle {
    pub(crate) fn new_texture(handle: HANDLE) -> Result<Self, GpuError> {
        Self::new(
            handle,
            GpuError::SharedTextureHandleCreationFailed(windows::core::Error::from_hresult(
                E_HANDLE,
            )),
        )
    }

    pub(crate) fn new_fence(handle: HANDLE) -> Result<Self, GpuError> {
        Self::new(
            handle,
            GpuError::SharedFenceHandleCreationFailed(windows::core::Error::from_hresult(E_HANDLE)),
        )
    }

    fn new(handle: HANDLE, invalid: GpuError) -> Result<Self, GpuError> {
        if handle.is_invalid() || handle == INVALID_HANDLE_VALUE {
            return Err(invalid);
        }
        Ok(Self(handle))
    }

    pub(crate) fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedNtHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: `self.0` is a valid handle opened by this owner and is closed exactly once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_not_clone<T>() {}

    #[test]
    fn owned_handle_is_not_clone() {
        assert_not_clone::<OwnedNtHandle>();
    }
}
