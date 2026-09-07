use repose_authdb_policy::ScreenSaverPolicy;

use crate::install_transaction::BackendError;

pub(crate) struct AuthorizationDb;

impl AuthorizationDb {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn read_screensaver(&self) -> Result<Vec<u8>, BackendError> {
        platform::read_screensaver()
    }

    pub(crate) fn write_screensaver(&self, policy: &ScreenSaverPolicy) -> Result<(), BackendError> {
        require_closed_production_gate()?;
        let bytes = policy
            .to_bytes()
            .map_err(|error| BackendError::new(error.to_string()))?;
        platform::write_screensaver(&bytes)
    }

    pub(crate) fn read_named_v1(&self) -> Result<Option<Vec<u8>>, BackendError> {
        platform::read_named_v1()
    }

    pub(crate) fn install_named_v1(&self) -> Result<(), BackendError> {
        require_closed_production_gate()?;
        platform::write_named_v1()
    }

    pub(crate) fn remove_named_v1(&self) -> Result<(), BackendError> {
        require_closed_production_gate()?;
        platform::remove_named_v1()
    }
}

fn require_closed_production_gate() -> Result<(), BackendError> {
    crate::production::verify_production_mutation_gate()
        .map_err(|error| BackendError::new(error.to_string()))
}

impl Default for AuthorizationDb {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::BackendError;

    fn unsupported<T>() -> Result<T, BackendError> {
        Err(BackendError::new(
            "Authorization Services is available only on macOS",
        ))
    }

    pub(super) fn read_screensaver() -> Result<Vec<u8>, BackendError> {
        unsupported()
    }
    pub(super) fn write_screensaver(_: &[u8]) -> Result<(), BackendError> {
        unsupported()
    }
    pub(super) fn read_named_v1() -> Result<Option<Vec<u8>>, BackendError> {
        unsupported()
    }
    pub(super) fn write_named_v1() -> Result<(), BackendError> {
        unsupported()
    }
    pub(super) fn remove_named_v1() -> Result<(), BackendError> {
        unsupported()
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod platform {
    use std::ffi::{c_int, c_void};
    use std::ptr;

    use super::BackendError;

    const MAXIMUM_DEFINITION_BYTES: usize = 1024 * 1024;

    unsafe extern "C" {
        fn repose_authdb_copy_screensaver(
            bytes: *mut *mut u8,
            length: *mut usize,
            found: *mut c_int,
        ) -> c_int;
        fn repose_authdb_set_screensaver(bytes: *const u8, length: usize) -> c_int;
        fn repose_authdb_copy_named_v1(
            bytes: *mut *mut u8,
            length: *mut usize,
            found: *mut c_int,
        ) -> c_int;
        fn repose_authdb_set_named_v1() -> c_int;
        fn repose_authdb_remove_named_v1() -> c_int;
        fn repose_authdb_free(bytes: *mut c_void);
    }

    pub(super) fn read_screensaver() -> Result<Vec<u8>, BackendError> {
        copy_definition(false, |bytes, length, found| {
            // SAFETY: initialized out-pointers remain valid for this call.
            unsafe { repose_authdb_copy_screensaver(bytes, length, found) }
        })?
        .ok_or_else(|| BackendError::new("screensaver right is missing"))
    }

    pub(super) fn write_screensaver(bytes: &[u8]) -> Result<(), BackendError> {
        // SAFETY: the slice is valid for the duration of the synchronous call.
        status(unsafe { repose_authdb_set_screensaver(bytes.as_ptr(), bytes.len()) })
    }

    pub(super) fn read_named_v1() -> Result<Option<Vec<u8>>, BackendError> {
        copy_definition(true, |bytes, length, found| {
            // SAFETY: initialized out-pointers remain valid for this call.
            unsafe { repose_authdb_copy_named_v1(bytes, length, found) }
        })
    }

    pub(super) fn write_named_v1() -> Result<(), BackendError> {
        // SAFETY: this adapter has no pointer arguments and constructs only its fixed rule.
        status(unsafe { repose_authdb_set_named_v1() })
    }

    pub(super) fn remove_named_v1() -> Result<(), BackendError> {
        // SAFETY: this adapter has no pointer arguments and names only its fixed right.
        status(unsafe { repose_authdb_remove_named_v1() })
    }

    fn copy_definition(
        missing_allowed: bool,
        call: impl FnOnce(*mut *mut u8, *mut usize, *mut c_int) -> c_int,
    ) -> Result<Option<Vec<u8>>, BackendError> {
        let mut pointer = ptr::null_mut();
        let mut length = 0_usize;
        let mut found = 0;
        let result = call(&mut pointer, &mut length, &mut found);
        if result != 0 {
            if !pointer.is_null() {
                // SAFETY: defensive cleanup of adapter-owned memory on error.
                unsafe { repose_authdb_free(pointer.cast::<c_void>()) };
            }
            return Err(BackendError::new(format!(
                "Authorization Services adapter failed ({result})"
            )));
        }
        if found == 0 {
            if !pointer.is_null() || length != 0 || !missing_allowed {
                if !pointer.is_null() {
                    // SAFETY: adapter-owned result buffer has not been adopted.
                    unsafe { repose_authdb_free(pointer.cast::<c_void>()) };
                }
                return Err(BackendError::new(
                    "Authorization Services returned an invalid missing result",
                ));
            }
            return Ok(None);
        }
        if found != 1 || length == 0 || length > MAXIMUM_DEFINITION_BYTES || pointer.is_null() {
            if !pointer.is_null() {
                // SAFETY: adapter-owned result buffer has not been adopted.
                unsafe { repose_authdb_free(pointer.cast::<c_void>()) };
            }
            return Err(BackendError::new(
                "Authorization Services returned invalid definition bytes",
            ));
        }
        // SAFETY: success guarantees `length` initialized malloc-owned bytes.
        let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
        // SAFETY: the adapter transferred this one allocation to the caller.
        unsafe { repose_authdb_free(pointer.cast::<c_void>()) };
        Ok(Some(bytes))
    }

    fn status(value: c_int) -> Result<(), BackendError> {
        if value == 0 {
            Ok(())
        } else {
            Err(BackendError::new(format!(
                "Authorization Services adapter failed ({value})"
            )))
        }
    }
}
