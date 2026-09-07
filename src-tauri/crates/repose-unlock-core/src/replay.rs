use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::protocol::crypto::AuthenticatedResponse;
use crate::protocol::messages::{DeviceId, MacId, PairingGeneration};
use crate::state_machine::{ChallengeVerified, SessionBinding};

static NEXT_GUARD_INSTANCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableCounterState {
    mac_id: MacId,
    device_id: DeviceId,
    pairing_generation: PairingGeneration,
    binding: SessionBinding,
    challenge_id: u64,
    counter: u64,
}

impl DurableCounterState {
    pub fn new(
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
        binding: SessionBinding,
        challenge_id: u64,
        counter: u64,
    ) -> Result<Self, DurableStateError> {
        if counter == 0 {
            return Err(DurableStateError::ZeroCounter);
        }
        Ok(Self {
            mac_id,
            device_id,
            pairing_generation,
            binding,
            challenge_id,
            counter,
        })
    }

    #[must_use]
    pub const fn mac_id(self) -> MacId {
        self.mac_id
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn pairing_generation(self) -> PairingGeneration {
        self.pairing_generation
    }

    #[must_use]
    pub const fn binding(self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn challenge_id(self) -> u64 {
        self.challenge_id
    }

    #[must_use]
    pub const fn counter(self) -> u64 {
        self.counter
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableRevocation {
    mac_id: MacId,
    device_id: DeviceId,
    pairing_generation: PairingGeneration,
}

impl DurableRevocation {
    #[must_use]
    pub const fn mac_id(self) -> MacId {
        self.mac_id
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn pairing_generation(self) -> PairingGeneration {
        self.pairing_generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableReplayState {
    Active(DurableCounterState),
    Revoked(DurableRevocation),
}

impl DurableReplayState {
    #[must_use]
    pub const fn mac_id(self) -> MacId {
        match self {
            Self::Active(value) => value.mac_id,
            Self::Revoked(value) => value.mac_id,
        }
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        match self {
            Self::Active(value) => value.device_id,
            Self::Revoked(value) => value.device_id,
        }
    }

    #[must_use]
    pub const fn pairing_generation(self) -> PairingGeneration {
        match self {
            Self::Active(value) => value.pairing_generation,
            Self::Revoked(value) => value.pairing_generation,
        }
    }
}

impl From<DurableCounterState> for DurableReplayState {
    fn from(value: DurableCounterState) -> Self {
        Self::Active(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableStateError {
    ZeroCounter,
    DuplicateDevice,
}

impl Display for DurableStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid durable replay state: {self:?}")
    }
}

impl Error for DurableStateError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStoreError {
    Unavailable,
    Corrupt,
}

impl Display for ReplayStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "durable replay store failed: {self:?}")
    }
}

impl Error for ReplayStoreError {}

pub trait CounterStore: Send + Sync {
    fn load(&self, device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError>;

    /// Atomically replace only the exact previously loaded value.
    ///
    /// Returning `Ok(true)` asserts that `replacement` is durable before return.
    fn compare_and_swap(
        &self,
        device_id: DeviceId,
        expected: Option<&DurableReplayState>,
        replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableSnapshot {
    entries: Vec<DurableReplayState>,
}

impl DurableSnapshot {
    #[must_use]
    pub fn entries(&self) -> &[DurableReplayState] {
        &self.entries
    }
}

/// Atomic in-memory implementation for tests and transient prototypes.
///
/// Production unlock service code must provide a [`CounterStore`] whose successful
/// compare-and-swap is durably persisted before it returns `true`.
#[derive(Clone)]
pub struct MemoryCounterStore {
    inner: Arc<Mutex<HashMap<DeviceId, DurableReplayState>>>,
}

impl MemoryCounterStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_entries<I, T>(entries: I) -> Result<Self, DurableStateError>
    where
        I: IntoIterator<Item = T>,
        T: Into<DurableReplayState>,
    {
        let store = Self::new();
        {
            let mut values = store.inner.lock();
            for entry in entries {
                let entry = entry.into();
                if values.insert(entry.device_id(), entry).is_some() {
                    return Err(DurableStateError::DuplicateDevice);
                }
            }
        }
        Ok(store)
    }

    pub fn from_snapshot(snapshot: DurableSnapshot) -> Result<Self, DurableStateError> {
        Self::from_entries(snapshot.entries)
    }

    #[must_use]
    pub fn snapshot(&self) -> DurableSnapshot {
        let mut entries: Vec<_> = self.inner.lock().values().copied().collect();
        entries.sort_by_key(|entry| *entry.device_id().as_bytes());
        DurableSnapshot { entries }
    }
}

impl Default for MemoryCounterStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterStore for MemoryCounterStore {
    fn load(&self, device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        Ok(self.inner.lock().get(&device_id).copied())
    }

    fn compare_and_swap(
        &self,
        device_id: DeviceId,
        expected: Option<&DurableReplayState>,
        replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        if replacement.device_id() != device_id {
            return Err(ReplayStoreError::Corrupt);
        }
        let mut values = self.inner.lock();
        if values.get(&device_id) != expected {
            return Ok(false);
        }
        values.insert(device_id, *replacement);
        Ok(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayPolicy {
    maximum_jump: u64,
}

impl ReplayPolicy {
    pub const DEFAULT_MAXIMUM_JUMP: u64 = 1_000_000;

    pub fn new(maximum_jump: u64) -> Result<Self, ReplayPolicyError> {
        if maximum_jump == 0 {
            return Err(ReplayPolicyError);
        }
        Ok(Self { maximum_jump })
    }
}

impl Default for ReplayPolicy {
    fn default() -> Self {
        Self {
            maximum_jump: Self::DEFAULT_MAXIMUM_JUMP,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayPolicyError;

impl Display for ReplayPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("maximum replay counter jump must be positive")
    }
}

impl Error for ReplayPolicyError {}

pub struct DurableReplayGuard<S> {
    store: S,
    policy: ReplayPolicy,
    instance_id: u64,
}

fn next_guard_instance_id() -> u64 {
    NEXT_GUARD_INSTANCE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .expect("durable replay guard instance space exhausted")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeOutcome {
    Revoked,
    AlreadyRevoked,
}

impl<S: CounterStore> DurableReplayGuard<S> {
    #[must_use]
    pub fn new(store: S, policy: ReplayPolicy) -> Self {
        Self {
            store,
            policy,
            instance_id: next_guard_instance_id(),
        }
    }

    pub fn revoke(
        &self,
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
    ) -> Result<RevokeOutcome, ReplayError> {
        let expected = self.store.load(device_id).map_err(ReplayError::Store)?;
        let replacement = DurableReplayState::Revoked(DurableRevocation {
            mac_id,
            device_id,
            pairing_generation,
        });
        match expected {
            None => {}
            Some(current) if current.mac_id() != mac_id || current.device_id() != device_id => {
                return Err(ReplayError::CorruptSnapshot);
            }
            Some(DurableReplayState::Active(current)) => {
                if current.pairing_generation > pairing_generation {
                    return Err(ReplayError::StaleGeneration);
                }
                if current.pairing_generation < pairing_generation {
                    return Err(ReplayError::GenerationRequiresRevocation);
                }
            }
            Some(DurableReplayState::Revoked(current)) => {
                if current.pairing_generation == pairing_generation {
                    return Ok(RevokeOutcome::AlreadyRevoked);
                }
                if current.pairing_generation > pairing_generation {
                    return Err(ReplayError::StaleGeneration);
                }
            }
        }
        if !self
            .store
            .compare_and_swap(device_id, expected.as_ref(), &replacement)
            .map_err(ReplayError::Store)?
        {
            return Err(ReplayError::ConcurrentUpdate);
        }
        Ok(RevokeOutcome::Revoked)
    }

    pub fn commit(
        &self,
        response: AuthenticatedResponse,
    ) -> Result<CommittedResponse, ReplayError> {
        let expected = self
            .store
            .load(response.device_id)
            .map_err(ReplayError::Store)?;
        let replacement = self.plan_commit(&response, expected)?;
        let intent = CommitIntent {
            guard_instance_id: self.instance_id,
            expected,
            replacement,
            response,
        };
        if !self
            .store
            .compare_and_swap(
                intent.replacement.device_id(),
                intent.expected.as_ref(),
                &intent.replacement,
            )
            .map_err(ReplayError::Store)?
        {
            return Err(ReplayError::ConcurrentUpdate);
        }
        let receipt = CommitReceipt {
            guard_instance_id: intent.guard_instance_id,
            expected: intent.expected,
            replacement: intent.replacement,
        };
        Ok(CommittedResponse {
            response: intent.response,
            receipt,
        })
    }

    pub fn finalize(&self, committed: CommittedResponse) -> Result<ChallengeVerified, ReplayError> {
        let CommittedResponse { response, receipt } = committed;
        if receipt.guard_instance_id != self.instance_id {
            return Err(ReplayError::WrongGuard);
        }
        let recomputed = self.plan_commit(&response, receipt.expected)?;
        if recomputed != receipt.replacement {
            return Err(ReplayError::ReceiptMismatch);
        }
        let persisted = self
            .store
            .load(response.device_id)
            .map_err(ReplayError::Store)?;
        if persisted != Some(receipt.replacement) {
            return Err(ReplayError::ReceiptMismatch);
        }
        Ok(ChallengeVerified::new(
            response.binding,
            response.challenge_id,
        ))
    }

    fn plan_commit(
        &self,
        response: &AuthenticatedResponse,
        expected: Option<DurableReplayState>,
    ) -> Result<DurableReplayState, ReplayError> {
        let previous_counter = match expected {
            None => 0,
            Some(current)
                if current.mac_id() != response.mac_id
                    || current.device_id() != response.device_id =>
            {
                return Err(ReplayError::CorruptSnapshot);
            }
            Some(DurableReplayState::Revoked(previous)) => {
                if response.pairing_generation <= previous.pairing_generation {
                    return Err(ReplayError::RevokedGeneration);
                }
                0
            }
            Some(DurableReplayState::Active(previous)) => {
                if response.pairing_generation > previous.pairing_generation {
                    return Err(ReplayError::GenerationRequiresRevocation);
                }
                if response.pairing_generation < previous.pairing_generation {
                    return Err(ReplayError::StaleGeneration);
                }
                if previous.counter >= response.counter {
                    return Err(ReplayError::NotAdvanced);
                }
                if previous.binding == response.binding
                    && previous.challenge_id == response.challenge_id.get()
                {
                    return Err(ReplayError::ChallengeAlreadyCommitted);
                }
                if previous.binding.lock_epoch() == response.binding.lock_epoch()
                    && previous.challenge_id >= response.challenge_id.get()
                {
                    return Err(ReplayError::StaleChallenge);
                }
                previous.counter
            }
        };
        let jump = response
            .counter
            .checked_sub(previous_counter)
            .ok_or(ReplayError::NotAdvanced)?;
        if jump > self.policy.maximum_jump {
            return Err(ReplayError::JumpTooLarge {
                previous: previous_counter,
                proposed: response.counter,
                maximum: self.policy.maximum_jump,
            });
        }
        Ok(DurableReplayState::Active(DurableCounterState {
            mac_id: response.mac_id,
            device_id: response.device_id,
            pairing_generation: response.pairing_generation,
            binding: response.binding,
            challenge_id: response.challenge_id.get(),
            counter: response.counter,
        }))
    }
}

struct CommitIntent {
    guard_instance_id: u64,
    expected: Option<DurableReplayState>,
    replacement: DurableReplayState,
    response: AuthenticatedResponse,
}

struct CommitReceipt {
    guard_instance_id: u64,
    expected: Option<DurableReplayState>,
    replacement: DurableReplayState,
}

/// Opaque evidence of a matching durable compare-and-swap.
///
/// ```compile_fail
/// use repose_unlock_core::replay::CommittedResponse;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<CommittedResponse>();
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::replay::CommittedResponse;
/// fn assert_copy<T: Copy>() {}
/// assert_copy::<CommittedResponse>();
/// ```
pub struct CommittedResponse {
    response: AuthenticatedResponse,
    receipt: CommitReceipt,
}

impl CommittedResponse {
    #[must_use]
    pub const fn mac_id(&self) -> MacId {
        self.response.mac_id
    }

    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        self.response.device_id
    }

    #[must_use]
    pub const fn pairing_generation(&self) -> PairingGeneration {
        self.response.pairing_generation
    }

    #[must_use]
    pub const fn binding(&self) -> SessionBinding {
        self.response.binding
    }

    #[must_use]
    pub const fn counter(&self) -> u64 {
        self.response.counter
    }
}

impl Debug for CommittedResponse {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("CommittedResponse(<opaque>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    Store(ReplayStoreError),
    CorruptSnapshot,
    NotAdvanced,
    JumpTooLarge {
        previous: u64,
        proposed: u64,
        maximum: u64,
    },
    ChallengeAlreadyCommitted,
    StaleChallenge,
    RevokedGeneration,
    GenerationRequiresRevocation,
    StaleGeneration,
    ConcurrentUpdate,
    WrongGuard,
    ReceiptMismatch,
}

impl Display for ReplayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "replay commitment failed: {self:?}")
    }
}

impl Error for ReplayError {}
