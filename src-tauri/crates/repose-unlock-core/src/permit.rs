use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use parking_lot::Mutex;

use crate::domain::MonoMillis;
use crate::replay::{CounterStore, DurableReplayGuard, GenerationAuthorityError};
use crate::state_machine::{ChallengeId, Permit, ReplayProvenance, SessionBinding};

pub struct PermitStore {
    inner: Mutex<Inner>,
}

struct Inner {
    permit: Option<StoredPermit>,
    last_observed_at: Option<MonoMillis>,
    next_handle: u64,
}

struct StoredPermit {
    handle: PermitHandle,
    permit: Permit,
    created_at: MonoMillis,
}

impl PermitStore {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                permit: None,
                last_observed_at: None,
                next_handle: 1,
            }),
        }
    }

    pub fn install(
        &self,
        permit: Permit,
        created_at: MonoMillis,
    ) -> Result<PermitHandle, InstallError> {
        if created_at >= permit.expires_at() {
            return Err(InstallError::InvalidLifetime);
        }
        let mut inner = self.inner.lock();
        if inner
            .last_observed_at
            .is_some_and(|previous| created_at < previous)
        {
            return Err(InstallError::NonMonotonicTime);
        }
        let next = inner
            .next_handle
            .checked_add(1)
            .ok_or(InstallError::HandleExhausted)?;
        let handle = PermitHandle(inner.next_handle);
        inner.next_handle = next;
        inner.last_observed_at = Some(created_at);
        inner.permit = Some(StoredPermit {
            handle,
            permit,
            created_at,
        });
        Ok(handle)
    }

    pub fn consume<S: CounterStore>(
        &self,
        authority: &DurableReplayGuard<S>,
        request_binding: SessionBinding,
        authoritative_binding: Option<SessionBinding>,
        now: MonoMillis,
    ) -> Result<ConsumedPermit, ConsumeError> {
        let mut inner = self.inner.lock();
        let Some(stored) = inner.permit.as_ref() else {
            return Err(ConsumeError::Empty);
        };
        if now < stored.created_at
            || inner
                .last_observed_at
                .is_some_and(|previous| now < previous)
        {
            inner.permit = None;
            return Err(ConsumeError::NonMonotonicTime);
        }
        inner.last_observed_at = Some(now);
        let stored = inner.permit.as_ref().expect("permit remains under lock");
        if now >= stored.permit.expires_at() {
            inner.permit = None;
            return Err(ConsumeError::Expired);
        }
        if authoritative_binding != Some(stored.permit.binding()) {
            inner.permit = None;
            return Err(ConsumeError::AuthoritativeSessionMismatch);
        }
        if let Err(error) = authority.validate_permit_authority(&stored.permit) {
            inner.permit = None;
            return Err(ConsumeError::Authority(error));
        }
        if request_binding != stored.permit.binding() {
            return Err(ConsumeError::RequestBindingMismatch);
        }
        let stored = inner.permit.take().expect("permit consumed under lock");
        Ok(ConsumedPermit::new(stored.permit))
    }

    pub fn expire_if(
        &self,
        handle: PermitHandle,
        authoritative_binding: Option<SessionBinding>,
        now: MonoMillis,
    ) -> Result<ExpireOutcome, ConsumeError> {
        let mut inner = self.inner.lock();
        let Some(stored) = inner.permit.as_ref() else {
            return Ok(ExpireOutcome::Empty);
        };
        if authoritative_binding != Some(stored.permit.binding()) {
            inner.permit = None;
            return Ok(ExpireOutcome::AuthoritativeSessionMismatch);
        }
        if stored.handle != handle {
            return Ok(ExpireOutcome::StaleHandle);
        }
        if now < stored.created_at
            || inner
                .last_observed_at
                .is_some_and(|previous| now < previous)
        {
            inner.permit = None;
            return Err(ConsumeError::NonMonotonicTime);
        }
        inner.last_observed_at = Some(now);
        let stored = inner.permit.as_ref().expect("matching permit remains");
        if now >= stored.permit.expires_at() {
            inner.permit = None;
            return Ok(ExpireOutcome::Expired);
        }
        Ok(ExpireOutcome::Retained)
    }

    /// Conditionally clear one permit generation. Delayed cleanup cannot remove a replacement.
    pub fn clear_if(&self, handle: PermitHandle) -> bool {
        let mut inner = self.inner.lock();
        if inner
            .permit
            .as_ref()
            .is_some_and(|stored| stored.handle == handle)
        {
            inner.permit = None;
            true
        } else {
            false
        }
    }

    pub fn clear(&self) {
        self.inner.lock().permit = None;
    }

    pub fn revoke(&self) {
        self.clear();
    }

    pub fn service_restarted(&self) {
        let mut inner = self.inner.lock();
        inner.permit = None;
        inner.last_observed_at = None;
    }
}

impl Default for PermitStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PermitHandle(u64);

/// The linear result of atomically consuming a permit.
///
/// Its tuple constructor is private, so external callers cannot forge it:
///
/// ```compile_fail
/// use repose_unlock_core::permit::ConsumedPermit;
/// let _ = ConsumedPermit;
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::permit::ConsumedPermit;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<ConsumedPermit>();
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::permit::ConsumedPermit;
/// fn assert_copy<T: Copy>() {}
/// assert_copy::<ConsumedPermit>();
/// ```
#[derive(PartialEq, Eq)]
pub struct ConsumedPermit(Permit);

impl ConsumedPermit {
    pub(crate) const fn new(permit: Permit) -> Self {
        Self(permit)
    }

    #[must_use]
    pub const fn binding(&self) -> SessionBinding {
        self.0.binding()
    }

    #[must_use]
    pub const fn challenge_id(&self) -> ChallengeId {
        self.0.challenge_id()
    }

    pub(crate) const fn provenance(&self) -> ReplayProvenance {
        self.0.provenance()
    }
}

impl Debug for ConsumedPermit {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("ConsumedPermit(<opaque>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallError {
    InvalidLifetime,
    NonMonotonicTime,
    HandleExhausted,
}

impl Display for InstallError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "permit installation failed: {self:?}")
    }
}

impl Error for InstallError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumeError {
    Empty,
    NonMonotonicTime,
    Expired,
    AuthoritativeSessionMismatch,
    RequestBindingMismatch,
    Authority(GenerationAuthorityError),
}

impl Display for ConsumeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "permit consumption failed: {self:?}")
    }
}

impl Error for ConsumeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpireOutcome {
    Empty,
    StaleHandle,
    Retained,
    Expired,
    AuthoritativeSessionMismatch,
}
