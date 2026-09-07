//! Durable phone-side response allocation and exact retransmission.
//!
//! The successful compare-and-swap is the counter/response linearization point.
//! A production store must make the exact replacement durable before returning
//! `true`; the coordinator also performs an exact read-back before releasing a
//! newly generated response.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::protocol::crypto::{
    CryptoRandom, PhoneBuildError, PhoneResponseSigner, VerifiedMacChallenge, build_phone_response,
    verify_phone_response_signature,
};
use crate::protocol::messages::{DeviceId, MacId, PairingGeneration, PublicKeyBytes};
use crate::protocol::wire::{CHALLENGE_FRAME_LEN, RESPONSE_FRAME_LEN};
use crate::state_machine::{ChallengeId, SessionBinding};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurablePhoneState {
    Ready(PhonePairingState),
    Cached(Box<CachedPhoneResponse>),
}

impl DurablePhoneState {
    #[must_use]
    pub const fn ready(
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
    ) -> Self {
        Self::Ready(PhonePairingState {
            mac_id,
            device_id,
            pairing_generation,
            counter: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_cached(
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
        binding: SessionBinding,
        challenge_id: u64,
        challenge_fingerprint: [u8; 32],
        counter: u64,
        response: [u8; RESPONSE_FRAME_LEN],
    ) -> Result<Self, PhoneStateError> {
        if counter == 0 {
            return Err(PhoneStateError::ZeroCounter);
        }
        let parsed = crate::protocol::wire::decode_response(&response)
            .map_err(|_| PhoneStateError::ResponseMismatch)?;
        if parsed.mac_id() != mac_id
            || parsed.device_id() != device_id
            || parsed.pairing_generation() != pairing_generation
            || parsed.binding() != binding
            || parsed.challenge_id().get() != challenge_id
            || parsed.counter() != counter
        {
            return Err(PhoneStateError::ResponseMismatch);
        }
        Ok(Self::Cached(Box::new(CachedPhoneResponse {
            mac_id,
            device_id,
            pairing_generation,
            binding,
            challenge_id: ChallengeId::from_protocol(challenge_id),
            challenge_fingerprint,
            counter,
            response,
        })))
    }

    #[must_use]
    pub const fn mac_id(&self) -> MacId {
        match self {
            Self::Ready(value) => value.mac_id,
            Self::Cached(value) => value.mac_id,
        }
    }

    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        match self {
            Self::Ready(value) => value.device_id,
            Self::Cached(value) => value.device_id,
        }
    }

    #[must_use]
    pub const fn pairing_generation(&self) -> PairingGeneration {
        match self {
            Self::Ready(value) => value.pairing_generation,
            Self::Cached(value) => value.pairing_generation,
        }
    }

    #[must_use]
    pub const fn counter(&self) -> u64 {
        match self {
            Self::Ready(value) => value.counter,
            Self::Cached(value) => value.counter,
        }
    }

    #[must_use]
    pub const fn binding(&self) -> Option<SessionBinding> {
        match self {
            Self::Ready(_) => None,
            Self::Cached(value) => Some(value.binding),
        }
    }

    #[must_use]
    pub const fn challenge_id(&self) -> Option<ChallengeId> {
        match self {
            Self::Ready(_) => None,
            Self::Cached(value) => Some(value.challenge_id),
        }
    }

    #[must_use]
    pub const fn challenge_fingerprint(&self) -> Option<&[u8; 32]> {
        match self {
            Self::Ready(_) => None,
            Self::Cached(value) => Some(&value.challenge_fingerprint),
        }
    }

    #[must_use]
    pub const fn response(&self) -> Option<&[u8; RESPONSE_FRAME_LEN]> {
        match self {
            Self::Ready(_) => None,
            Self::Cached(value) => Some(&value.response),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonePairingState {
    mac_id: MacId,
    device_id: DeviceId,
    pairing_generation: PairingGeneration,
    counter: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPhoneResponse {
    mac_id: MacId,
    device_id: DeviceId,
    pairing_generation: PairingGeneration,
    binding: SessionBinding,
    challenge_id: ChallengeId,
    challenge_fingerprint: [u8; 32],
    counter: u64,
    response: [u8; RESPONSE_FRAME_LEN],
}

impl CachedPhoneResponse {
    #[allow(clippy::too_many_arguments)]
    fn matches_intent(
        &self,
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
        binding: SessionBinding,
        challenge_id: ChallengeId,
        challenge_fingerprint: &[u8; 32],
        counter: u64,
        mac_nonce: &[u8; 32],
        mac_ephemeral_public_key: PublicKeyBytes,
        challenge_frame: &[u8; CHALLENGE_FRAME_LEN],
        phone_identity_public_key: PublicKeyBytes,
    ) -> bool {
        let Ok(response) = crate::protocol::wire::decode_response(&self.response) else {
            return false;
        };
        self.mac_id == mac_id
            && self.device_id == device_id
            && self.pairing_generation == pairing_generation
            && self.binding == binding
            && self.challenge_id == challenge_id
            && &self.challenge_fingerprint == challenge_fingerprint
            && self.counter == counter
            && response.mac_id() == mac_id
            && response.device_id() == device_id
            && response.pairing_generation() == pairing_generation
            && response.binding() == binding
            && response.challenge_id() == challenge_id
            && response.counter() == counter
            && response.mac_nonce() == mac_nonce
            && response.mac_ephemeral_public_key() == mac_ephemeral_public_key
            && verify_phone_response_signature(
                challenge_frame,
                &response,
                phone_identity_public_key,
            )
            .is_ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneStoreError {
    Unavailable,
    Corrupt,
}

impl Display for PhoneStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "durable phone response store failed: {self:?}")
    }
}

impl Error for PhoneStoreError {}

pub trait PhoneResponseStore: Send + Sync {
    fn load(
        &self,
        mac_id: MacId,
        device_id: DeviceId,
    ) -> Result<Option<DurablePhoneState>, PhoneStoreError>;

    /// Atomically replace only the exact previously loaded state.
    ///
    /// `Ok(true)` guarantees that the exact replacement, including cached
    /// response bytes, is durable before this method returns.
    fn compare_and_swap(
        &self,
        mac_id: MacId,
        device_id: DeviceId,
        expected: Option<&DurablePhoneState>,
        replacement: &DurablePhoneState,
    ) -> Result<bool, PhoneStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneSnapshot {
    entries: Vec<DurablePhoneState>,
}

impl PhoneSnapshot {
    #[must_use]
    pub fn entries(&self) -> &[DurablePhoneState] {
        &self.entries
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneStateError {
    DuplicatePairing,
    ZeroCounter,
    ResponseMismatch,
}

impl Display for PhoneStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid durable phone response state")
    }
}

impl Error for PhoneStateError {}

#[derive(Clone)]
pub struct MemoryPhoneResponseStore {
    inner: Arc<Mutex<HashMap<(MacId, DeviceId), DurablePhoneState>>>,
}

impl MemoryPhoneResponseStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_snapshot(snapshot: PhoneSnapshot) -> Result<Self, PhoneStateError> {
        let store = Self::new();
        {
            let mut values = store.inner.lock();
            for entry in snapshot.entries {
                let key = (entry.mac_id(), entry.device_id());
                if values.insert(key, entry).is_some() {
                    return Err(PhoneStateError::DuplicatePairing);
                }
            }
        }
        Ok(store)
    }

    #[must_use]
    pub fn snapshot(&self) -> PhoneSnapshot {
        let mut entries: Vec<_> = self.inner.lock().values().cloned().collect();
        entries.sort_by_key(|entry| (*entry.mac_id().as_bytes(), *entry.device_id().as_bytes()));
        PhoneSnapshot { entries }
    }
}

impl Default for MemoryPhoneResponseStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PhoneResponseStore for MemoryPhoneResponseStore {
    fn load(
        &self,
        mac_id: MacId,
        device_id: DeviceId,
    ) -> Result<Option<DurablePhoneState>, PhoneStoreError> {
        Ok(self.inner.lock().get(&(mac_id, device_id)).cloned())
    }

    fn compare_and_swap(
        &self,
        mac_id: MacId,
        device_id: DeviceId,
        expected: Option<&DurablePhoneState>,
        replacement: &DurablePhoneState,
    ) -> Result<bool, PhoneStoreError> {
        if replacement.mac_id() != mac_id || replacement.device_id() != device_id {
            return Err(PhoneStoreError::Corrupt);
        }
        let mut values = self.inner.lock();
        let key = (mac_id, device_id);
        if values.get(&key) != expected {
            return Ok(false);
        }
        values.insert(key, replacement.clone());
        Ok(true)
    }
}

pub struct PhoneResponseCoordinator<S> {
    store: S,
    gate: Mutex<()>,
}

impl<S: PhoneResponseStore> PhoneResponseCoordinator<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self {
            store,
            gate: Mutex::new(()),
        }
    }

    /// Atomically return a cached response or allocate, sign, and durably cache one.
    ///
    /// A raw or merely parsed Challenge cannot enter this API:
    ///
    /// ```compile_fail
    /// use repose_unlock_core::phone::{MemoryPhoneResponseStore, PhoneResponseCoordinator};
    /// use repose_unlock_core::protocol::crypto::{CryptoRandom, PhoneResponseSigner};
    /// use repose_unlock_core::protocol::messages::Challenge;
    /// fn cannot_respond<R: CryptoRandom, S: PhoneResponseSigner>(
    ///     coordinator: &PhoneResponseCoordinator<MemoryPhoneResponseStore>,
    ///     raw: Challenge,
    ///     rng: &mut R,
    ///     signer: &mut S,
    /// ) {
    ///     let _ = coordinator.respond(raw, rng, signer);
    /// }
    /// ```
    pub fn respond<R: CryptoRandom, I: PhoneResponseSigner>(
        &self,
        challenge: VerifiedMacChallenge,
        rng: &mut R,
        signer: &mut I,
    ) -> Result<[u8; RESPONSE_FRAME_LEN], PhoneResponseError> {
        let _gate = self.gate.lock();
        let challenge_frame = *challenge.frame();
        let phone_identity_public_key = challenge.phone_identity_public_key();
        let message = challenge.message();
        let mac_id = message.mac_id();
        let device_id = message.device_id();
        let fingerprint: [u8; 32] = Sha256::digest(challenge.frame()).into();
        let expected = self
            .store
            .load(mac_id, device_id)
            .map_err(PhoneResponseError::Store)?;
        let previous_counter = match expected.as_ref() {
            None => return Err(PhoneResponseError::UninitializedPairing),
            Some(current)
                if current.mac_id() != message.mac_id()
                    || current.device_id() != message.device_id() =>
            {
                return Err(PhoneResponseError::PairingMismatch);
            }
            Some(current) if current.pairing_generation() != message.pairing_generation() => {
                return Err(PhoneResponseError::PairingRotationRequired);
            }
            Some(DurablePhoneState::Ready(current)) => current.counter,
            Some(DurablePhoneState::Cached(current)) => {
                if current.challenge_fingerprint == fingerprint {
                    if !current.matches_intent(
                        mac_id,
                        device_id,
                        message.pairing_generation(),
                        message.binding(),
                        message.challenge_id(),
                        &fingerprint,
                        current.counter,
                        message.mac_nonce(),
                        message.mac_ephemeral_public_key(),
                        &challenge_frame,
                        phone_identity_public_key,
                    ) {
                        return Err(PhoneResponseError::CorruptSnapshot);
                    }
                    return Ok(current.response);
                }
                let previous_epoch = current.binding.lock_epoch();
                let proposed_epoch = message.binding().lock_epoch();
                if proposed_epoch < previous_epoch
                    || (proposed_epoch == previous_epoch
                        && message.challenge_id().get() < current.challenge_id.get())
                {
                    return Err(PhoneResponseError::StaleChallenge);
                }
                if proposed_epoch == previous_epoch
                    && (message.binding() != current.binding
                        || message.challenge_id() == current.challenge_id)
                {
                    return Err(PhoneResponseError::ChallengeConflict);
                }
                current.counter
            }
        };
        let expected_counter = previous_counter
            .max(message.counter_floor())
            .checked_add(1)
            .ok_or(PhoneResponseError::CounterOverflow)?;
        let binding = message.binding();
        let challenge_id = message.challenge_id();
        let generation = message.pairing_generation();
        let mac_nonce = *message.mac_nonce();
        let mac_ephemeral_public_key = message.mac_ephemeral_public_key();
        let response = build_phone_response(challenge, previous_counter, rng, signer)
            .map_err(PhoneResponseError::Build)?;
        let counter = crate::protocol::wire::decode_response(&response)
            .map_err(|_| PhoneResponseError::CorruptSnapshot)?
            .counter();
        if counter != expected_counter {
            return Err(PhoneResponseError::CorruptSnapshot);
        }
        let replacement = DurablePhoneState::Cached(Box::new(CachedPhoneResponse {
            mac_id,
            device_id,
            pairing_generation: generation,
            binding,
            challenge_id,
            challenge_fingerprint: fingerprint,
            counter,
            response,
        }));
        if self
            .store
            .compare_and_swap(mac_id, device_id, expected.as_ref(), &replacement)
            .map_err(PhoneResponseError::Store)?
        {
            return match self
                .store
                .load(mac_id, device_id)
                .map_err(PhoneResponseError::Store)?
            {
                Some(persisted) if persisted == replacement => Ok(response),
                _ => Err(PhoneResponseError::CommitMismatch),
            };
        }
        match self
            .store
            .load(mac_id, device_id)
            .map_err(PhoneResponseError::Store)?
        {
            Some(DurablePhoneState::Cached(winner))
                if winner.matches_intent(
                    mac_id,
                    device_id,
                    generation,
                    binding,
                    challenge_id,
                    &fingerprint,
                    expected_counter,
                    &mac_nonce,
                    mac_ephemeral_public_key,
                    &challenge_frame,
                    phone_identity_public_key,
                ) =>
            {
                Ok(winner.response)
            }
            _ => Err(PhoneResponseError::ConcurrentUpdate),
        }
    }

    pub fn rotate_pairing(
        &self,
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
    ) -> Result<PairingRotateOutcome, PhoneResponseError> {
        let _gate = self.gate.lock();
        let expected = self
            .store
            .load(mac_id, device_id)
            .map_err(PhoneResponseError::Store)?;
        let (counter, outcome) = match expected.as_ref() {
            None => (0, PairingRotateOutcome::Initialized),
            Some(current) if current.mac_id() != mac_id || current.device_id() != device_id => {
                return Err(PhoneResponseError::PairingMismatch);
            }
            Some(current) if current.pairing_generation() == pairing_generation => {
                return Ok(PairingRotateOutcome::AlreadyCurrent);
            }
            Some(current) if current.pairing_generation() > pairing_generation => {
                return Err(PhoneResponseError::StalePairingGeneration);
            }
            Some(_) => (0, PairingRotateOutcome::Rotated),
        };
        let replacement = DurablePhoneState::Ready(PhonePairingState {
            mac_id,
            device_id,
            pairing_generation,
            counter,
        });
        if !self
            .store
            .compare_and_swap(mac_id, device_id, expected.as_ref(), &replacement)
            .map_err(PhoneResponseError::Store)?
        {
            return Err(PhoneResponseError::ConcurrentUpdate);
        }
        Ok(outcome)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingRotateOutcome {
    Initialized,
    Rotated,
    AlreadyCurrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneResponseError {
    Store(PhoneStoreError),
    UninitializedPairing,
    PairingMismatch,
    PairingRotationRequired,
    StalePairingGeneration,
    StaleChallenge,
    ChallengeConflict,
    CounterOverflow,
    ConcurrentUpdate,
    CommitMismatch,
    CorruptSnapshot,
    Build(PhoneBuildError),
}

impl Display for PhoneResponseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "phone response coordination failed: {self:?}")
    }
}

impl Error for PhoneResponseError {}
