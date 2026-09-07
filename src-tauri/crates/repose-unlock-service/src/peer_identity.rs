use std::error::Error;
use std::ffi::c_void;
use std::fmt::{self, Display, Formatter};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::ptr::NonNull;

use repose_unlock_core::domain::AuditSessionId;

pub(crate) const AUTHORIZATIONHOST_REQUIREMENT: &str =
    "identifier \"com.apple.authorizationhost\" and anchor apple";

#[derive(Clone, Copy)]
pub(crate) struct VerifiedPeer {
    effective_uid: u32,
    audit_session_id: AuditSessionId,
    process_id: i32,
    process_version: u32,
}

impl VerifiedPeer {
    pub(crate) const fn effective_uid(self) -> u32 {
        self.effective_uid
    }

    pub(crate) const fn audit_session_id(self) -> AuditSessionId {
        self.audit_session_id
    }

    #[allow(dead_code)]
    pub(crate) const fn process_id(self) -> i32 {
        self.process_id
    }

    #[allow(dead_code)]
    pub(crate) const fn process_version(self) -> u32 {
        self.process_version
    }

    #[cfg(debug_assertions)]
    pub(crate) const fn for_test(
        effective_uid: u32,
        audit_session_id: AuditSessionId,
        process_id: i32,
        process_version: u32,
    ) -> Self {
        Self {
            effective_uid,
            audit_session_id,
            process_id,
            process_version,
        }
    }
}

pub(crate) trait PeerVerifier: Send + Sync {
    fn verify(&self, stream: &UnixStream) -> Result<VerifiedPeer, PeerIdentityError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerIdentityError {
    Initialization,
    Rejected,
}

impl Display for PeerIdentityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("authorizationhost peer identity rejected")
    }
}

impl Error for PeerIdentityError {}

#[cfg(target_os = "macos")]
pub(crate) struct MacOsAuthorizationHostVerifier {
    handle: NonNull<c_void>,
}

// SAFETY: the handle owns one immutable SecRequirementRef. Security.framework
// validity checks only borrow it, and destruction happens after the last shared
// production verifier reference is dropped.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
unsafe impl Send for MacOsAuthorizationHostVerifier {}

// SAFETY: see the Send rationale; concurrent checks do not mutate the retained
// requirement or transfer its ownership.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
unsafe impl Sync for MacOsAuthorizationHostVerifier {}

#[cfg(target_os = "macos")]
impl MacOsAuthorizationHostVerifier {
    pub(crate) fn new() -> Result<Self, PeerIdentityError> {
        sys::create().map(|handle| Self { handle })
    }
}

#[cfg(target_os = "macos")]
impl PeerVerifier for MacOsAuthorizationHostVerifier {
    fn verify(&self, stream: &UnixStream) -> Result<VerifiedPeer, PeerIdentityError> {
        let claims = sys::verify(self.handle, stream.as_raw_fd())?;
        Ok(VerifiedPeer {
            effective_uid: claims.effective_uid,
            audit_session_id: AuditSessionId::new(claims.audit_session_id),
            process_id: claims.process_id,
            process_version: claims.process_version,
        })
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacOsAuthorizationHostVerifier {
    fn drop(&mut self) {
        sys::destroy(self.handle);
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod sys {
    use std::ffi::c_void;
    use std::os::raw::c_int;
    use std::ptr::NonNull;

    use super::PeerIdentityError;

    #[repr(C)]
    pub(super) struct Claims {
        pub(super) effective_uid: u32,
        pub(super) audit_session_id: u32,
        pub(super) process_id: i32,
        pub(super) process_version: u32,
    }

    unsafe extern "C" {
        fn repose_peer_verifier_create(requirement: *const u8, length: usize) -> *mut c_void;
        fn repose_peer_verifier_verify(
            verifier: *mut c_void,
            socket_fd: c_int,
            claims: *mut Claims,
        ) -> c_int;
        fn repose_peer_verifier_destroy(verifier: *mut c_void);
    }

    pub(super) fn create() -> Result<NonNull<c_void>, PeerIdentityError> {
        let requirement = super::AUTHORIZATIONHOST_REQUIREMENT.as_bytes();
        // SAFETY: the C function borrows this exact byte slice only for the call.
        NonNull::new(unsafe {
            repose_peer_verifier_create(requirement.as_ptr(), requirement.len())
        })
        .ok_or(PeerIdentityError::Initialization)
    }

    pub(super) fn verify(
        handle: NonNull<c_void>,
        socket_fd: c_int,
    ) -> Result<Claims, PeerIdentityError> {
        let mut claims = Claims {
            effective_uid: 0,
            audit_session_id: 0,
            process_id: 0,
            process_version: 0,
        };
        // SAFETY: `handle` originates from create and remains owned by the verifier;
        // the output points to initialized, correctly sized writable storage.
        let status =
            unsafe { repose_peer_verifier_verify(handle.as_ptr(), socket_fd, &mut claims) };
        if status == 0 {
            Ok(claims)
        } else {
            Err(PeerIdentityError::Rejected)
        }
    }

    pub(super) fn destroy(handle: NonNull<c_void>) {
        // SAFETY: Drop calls this exactly once for the owned create result.
        unsafe { repose_peer_verifier_destroy(handle.as_ptr()) };
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::os::unix::net::UnixStream;

    use super::{MacOsAuthorizationHostVerifier, PeerIdentityError, PeerVerifier};

    #[test]
    fn concrete_verifier_rejects_this_non_authorizationhost_process() {
        let verifier = MacOsAuthorizationHostVerifier::new().expect("compile fixed requirement");
        let (accepted_end, _client_end) = UnixStream::pair().expect("local socket pair");
        assert!(matches!(
            verifier.verify(&accepted_end),
            Err(PeerIdentityError::Rejected)
        ));
    }
}
