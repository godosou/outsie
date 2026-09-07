use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionBinding {
    lock_epoch: LockEpoch,
    audit_session_id: AuditSessionId,
    console_uid: ConsoleUid,
}

impl SessionBinding {
    #[must_use]
    pub const fn new(
        lock_epoch: LockEpoch,
        audit_session_id: AuditSessionId,
        console_uid: ConsoleUid,
    ) -> Self {
        Self {
            lock_epoch,
            audit_session_id,
            console_uid,
        }
    }

    #[must_use]
    pub const fn lock_epoch(self) -> LockEpoch {
        self.lock_epoch
    }

    #[must_use]
    pub const fn audit_session_id(self) -> AuditSessionId {
        self.audit_session_id
    }

    #[must_use]
    pub const fn console_uid(self) -> ConsoleUid {
        self.console_uid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimingPolicy {
    challenge_ttl_ms: u64,
    permit_ttl_ms: u64,
    cooldown_ms: u64,
}

impl TimingPolicy {
    pub const MAX_CHALLENGE_TTL_MS: u64 = 60_000;
    pub const MAX_PERMIT_TTL_MS: u64 = 10_000;
    pub const MAX_COOLDOWN_MS: u64 = 300_000;

    pub fn new(
        challenge_ttl_ms: u64,
        permit_ttl_ms: u64,
        cooldown_ms: u64,
    ) -> Result<Self, TimingPolicyError> {
        for (field, value, maximum) in [
            (
                TimingField::ChallengeTtl,
                challenge_ttl_ms,
                Self::MAX_CHALLENGE_TTL_MS,
            ),
            (
                TimingField::PermitTtl,
                permit_ttl_ms,
                Self::MAX_PERMIT_TTL_MS,
            ),
            (TimingField::Cooldown, cooldown_ms, Self::MAX_COOLDOWN_MS),
        ] {
            if value == 0 {
                return Err(TimingPolicyError::Zero { field });
            }
            if value > maximum {
                return Err(TimingPolicyError::ExceedsMaximum {
                    field,
                    value,
                    maximum,
                });
            }
        }
        Ok(Self {
            challenge_ttl_ms,
            permit_ttl_ms,
            cooldown_ms,
        })
    }

    #[must_use]
    pub const fn challenge_ttl_ms(self) -> u64 {
        self.challenge_ttl_ms
    }

    #[must_use]
    pub const fn permit_ttl_ms(self) -> u64 {
        self.permit_ttl_ms
    }

    #[must_use]
    pub const fn cooldown_ms(self) -> u64 {
        self.cooldown_ms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingField {
    ChallengeTtl,
    PermitTtl,
    Cooldown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingPolicyError {
    Zero {
        field: TimingField,
    },
    ExceedsMaximum {
        field: TimingField,
        value: u64,
        maximum: u64,
    },
}

impl Display for TimingPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero { field } => write!(formatter, "{field:?} must be positive"),
            Self::ExceedsMaximum {
                field,
                value,
                maximum,
            } => write!(
                formatter,
                "{field:?} value {value} exceeds maximum {maximum} milliseconds"
            ),
        }
    }
}

impl Error for TimingPolicyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChallengeId(u64);

impl ChallengeId {
    #[must_use]
    const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn from_protocol(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The one-shot command data for starting or cancelling a challenge worker.
///
/// Challenge commands deliberately cannot be cloned:
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::ChallengeRequest;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<ChallengeRequest>();
/// ```
///
/// Nor can they be copied:
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::ChallengeRequest;
/// fn assert_copy<T: Copy>() {}
/// assert_copy::<ChallengeRequest>();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct ChallengeRequest {
    binding: SessionBinding,
    challenge_id: ChallengeId,
    deadline: MonoMillis,
}

impl ChallengeRequest {
    #[must_use]
    pub const fn binding(&self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn challenge_id(&self) -> ChallengeId {
        self.challenge_id
    }

    #[must_use]
    pub const fn deadline(&self) -> MonoMillis {
        self.deadline
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChallengeRecord {
    binding: SessionBinding,
    challenge_id: ChallengeId,
    deadline: MonoMillis,
}

impl ChallengeRecord {
    const fn new(binding: SessionBinding, challenge_id: ChallengeId, deadline: MonoMillis) -> Self {
        Self {
            binding,
            challenge_id,
            deadline,
        }
    }

    const fn command(self) -> ChallengeRequest {
        ChallengeRequest {
            binding: self.binding,
            challenge_id: self.challenge_id,
            deadline: self.deadline,
        }
    }
}

/// Evidence that the protocol layer has authenticated a response for one challenge.
///
/// The fields and constructor are intentionally not public. External callers can
/// submit this type through [`Event::ChallengeVerified`] after a verifier in this
/// crate creates it, but cannot manufacture verified evidence through the safe API.
/// It is also a linear capability and cannot be duplicated:
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::ChallengeVerified;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<ChallengeVerified>();
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::ChallengeVerified;
/// fn assert_copy<T: Copy>() {}
/// assert_copy::<ChallengeVerified>();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct ChallengeVerified {
    binding: SessionBinding,
    challenge_id: ChallengeId,
}

impl ChallengeVerified {
    #[allow(dead_code)]
    pub(crate) const fn new(binding: SessionBinding, challenge_id: ChallengeId) -> Self {
        Self {
            binding,
            challenge_id,
        }
    }
}

/// A linear one-shot command authorizing installation in the permit store.
///
/// It cannot be duplicated through the safe API:
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::Permit;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<Permit>();
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::Permit;
/// fn assert_copy<T: Copy>() {}
/// assert_copy::<Permit>();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct Permit {
    binding: SessionBinding,
    challenge_id: ChallengeId,
    expires_at: MonoMillis,
}

impl Permit {
    #[must_use]
    pub const fn binding(&self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn challenge_id(&self) -> ChallengeId {
        self.challenge_id
    }

    #[must_use]
    pub const fn expires_at(&self) -> MonoMillis {
        self.expires_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PermitRecord {
    binding: SessionBinding,
    challenge_id: ChallengeId,
    expires_at: MonoMillis,
}

impl PermitRecord {
    const fn command(self) -> Permit {
        Permit {
            binding: self.binding,
            challenge_id: self.challenge_id,
            expires_at: self.expires_at,
        }
    }
}

/// A one-shot command emitted by the pure reducer.
///
/// Effects cannot be cloned into duplicate command executions:
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::Effect;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<Effect>();
/// ```
///
/// Effects also cannot be implicitly copied:
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::Effect;
/// fn assert_copy<T: Copy>() {}
/// assert_copy::<Effect>();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    StartChallenge(ChallengeRequest),
    CancelChallenge(ChallengeRequest),
    AbortAllChallenges,
    CreatePermit(Permit),
    ClearPermit,
}

impl Effect {
    #[must_use]
    pub const fn is_start_challenge(&self) -> bool {
        matches!(self, Self::StartChallenge(_))
    }

    #[must_use]
    pub const fn is_create_permit(&self) -> bool {
        matches!(self, Self::CreatePermit(_))
    }

    #[must_use]
    pub const fn is_cancel_challenge(&self) -> bool {
        matches!(self, Self::CancelChallenge(_))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    SessionLocked {
        binding: SessionBinding,
    },
    FarStable {
        binding: SessionBinding,
    },
    ReliableDisconnect {
        binding: SessionBinding,
    },
    NearStable {
        binding: SessionBinding,
    },
    ChallengeVerified(ChallengeVerified),
    ChallengeFailed {
        binding: SessionBinding,
        challenge_id: ChallengeId,
    },
    ChallengeTimedOut {
        binding: SessionBinding,
        challenge_id: ChallengeId,
    },
    ChallengeTerminated {
        binding: SessionBinding,
        challenge_id: ChallengeId,
    },
    PermitConsumed {
        binding: SessionBinding,
        challenge_id: ChallengeId,
    },
    PermitExpired {
        binding: SessionBinding,
        challenge_id: ChallengeId,
    },
    SessionUnlocked,
    Logout,
    FastUserSwitch {
        locked_binding: Option<SessionBinding>,
    },
    ServiceRestarted {
        locked_binding: Option<SessionBinding>,
    },
    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockPhase {
    Unlocked,
    LockedUnarmed,
    LockedArmed,
    Challenging,
    Cancelling,
    PermitReady,
    Unlocking,
}

/// The linear state token consumed by [`transition`].
///
/// Safe callers cannot fork the reducer by cloning its state:
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::{TimingPolicy, UnlockState};
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<UnlockState>();
/// # let _ = TimingPolicy::new(5_000, 3_000, 2_000);
/// ```
///
/// The state token is not copyable either:
///
/// ```compile_fail
/// use repose_unlock_core::state_machine::UnlockState;
/// fn assert_copy<T: Copy>() {}
/// assert_copy::<UnlockState>();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct UnlockState {
    state: StateData,
    last_observed_at: Option<MonoMillis>,
    authoritative_binding: Option<SessionBinding>,
    last_challenge_id: u64,
    timing_policy: TimingPolicy,
    worker_fence: Option<WorkerFence>,
}

#[derive(Debug, PartialEq, Eq)]
enum StateData {
    Unlocked,
    LockedUnarmed {
        binding: SessionBinding,
    },
    LockedArmed {
        binding: SessionBinding,
        cooldown_until: Option<MonoMillis>,
    },
    Challenging {
        request: ChallengeRecord,
    },
    Cancelling {
        request: ChallengeRecord,
        cooldown_until: Option<MonoMillis>,
    },
    PermitReady {
        permit: PermitRecord,
    },
    Unlocking {
        binding: SessionBinding,
    },
}

impl UnlockState {
    #[must_use]
    pub const fn unlocked(timing_policy: TimingPolicy) -> Self {
        Self {
            state: StateData::Unlocked,
            last_observed_at: None,
            authoritative_binding: None,
            last_challenge_id: 0,
            timing_policy,
            worker_fence: None,
        }
    }

    #[must_use]
    pub const fn phase(&self) -> UnlockPhase {
        match &self.state {
            StateData::Unlocked => UnlockPhase::Unlocked,
            StateData::LockedUnarmed { .. } => UnlockPhase::LockedUnarmed,
            StateData::LockedArmed { .. } => UnlockPhase::LockedArmed,
            StateData::Challenging { .. } => UnlockPhase::Challenging,
            StateData::Cancelling { .. } => UnlockPhase::Cancelling,
            StateData::PermitReady { .. } => UnlockPhase::PermitReady,
            StateData::Unlocking { .. } => UnlockPhase::Unlocking,
        }
    }

    #[must_use]
    pub const fn binding(&self) -> Option<SessionBinding> {
        match &self.state {
            StateData::Unlocked => None,
            StateData::LockedUnarmed { binding }
            | StateData::LockedArmed { binding, .. }
            | StateData::Unlocking { binding } => Some(*binding),
            StateData::Challenging { request } => Some(request.binding),
            StateData::Cancelling { request, .. } => Some(request.binding),
            StateData::PermitReady { permit } => Some(permit.binding),
        }
    }

    #[must_use]
    pub const fn challenge_id(&self) -> Option<ChallengeId> {
        match &self.state {
            StateData::Challenging { request } => Some(request.challenge_id),
            StateData::Cancelling { request, .. } => Some(request.challenge_id),
            StateData::PermitReady { permit } => Some(permit.challenge_id),
            _ => None,
        }
    }

    #[must_use]
    pub const fn cooldown_until(&self) -> Option<MonoMillis> {
        match &self.state {
            StateData::LockedArmed { cooldown_until, .. }
            | StateData::Cancelling { cooldown_until, .. } => *cooldown_until,
            _ => None,
        }
    }

    #[must_use]
    pub const fn permit_expires_at(&self) -> Option<MonoMillis> {
        match &self.state {
            StateData::PermitReady { permit } => Some(permit.expires_at),
            _ => None,
        }
    }

    #[must_use]
    pub const fn last_observed_at(&self) -> Option<MonoMillis> {
        self.last_observed_at
    }

    #[must_use]
    pub const fn highest_lock_epoch(&self) -> Option<LockEpoch> {
        match self.authoritative_binding {
            Some(binding) => Some(binding.lock_epoch),
            None => None,
        }
    }

    #[must_use]
    pub const fn authoritative_binding(&self) -> Option<SessionBinding> {
        self.authoritative_binding
    }

    #[must_use]
    pub const fn timing_policy(&self) -> TimingPolicy {
        self.timing_policy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkerFence {
    binding: SessionBinding,
    challenge_id: ChallengeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionErrorKind {
    NonMonotonicTime {
        previous: MonoMillis,
        current: MonoMillis,
    },
    TimeOverflow {
        operation: TimingOperation,
        now: MonoMillis,
        duration_ms: u64,
    },
    ChallengeIdExhausted,
}

impl Display for TransitionErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonMonotonicTime { previous, current } => write!(
                formatter,
                "monotonic time moved backwards from {} to {}",
                previous.get(),
                current.get()
            ),
            Self::TimeOverflow {
                operation,
                now,
                duration_ms,
            } => write!(
                formatter,
                "{operation:?} overflows at {} plus {duration_ms} milliseconds",
                now.get()
            ),
            Self::ChallengeIdExhausted => write!(formatter, "challenge identifier exhausted"),
        }
    }
}

impl Error for TransitionErrorKind {}

/// A failed transition together with the unchanged authoritative reducer state.
///
/// The error deliberately is not cloneable: callers recover the unique state token
/// with [`Self::into_state`] or [`Self::into_parts`] before deciding how to proceed.
#[derive(Debug, PartialEq, Eq)]
pub struct TransitionError {
    kind: TransitionErrorKind,
    state: Box<UnlockState>,
}

impl TransitionError {
    fn new(state: UnlockState, kind: TransitionErrorKind) -> Self {
        Self {
            kind,
            state: Box::new(state),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> TransitionErrorKind {
        self.kind
    }

    #[must_use]
    pub fn into_state(self) -> UnlockState {
        *self.state
    }

    #[must_use]
    pub fn into_parts(self) -> (UnlockState, TransitionErrorKind) {
        (*self.state, self.kind)
    }
}

impl Display for TransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.kind.fmt(formatter)
    }
}

impl Error for TransitionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingOperation {
    ChallengeDeadline,
    PermitExpiry,
    CooldownDeadline,
}

pub fn transition(
    state: UnlockState,
    event: Event,
    now: MonoMillis,
) -> Result<(UnlockState, Vec<Effect>), TransitionError> {
    match event {
        Event::SessionLocked { binding } => {
            return Ok(reset_locked(state, binding, now));
        }
        Event::SessionUnlocked | Event::Logout => {
            return Ok(reset_unlocked(state, now));
        }
        Event::FastUserSwitch { locked_binding } | Event::ServiceRestarted { locked_binding } => {
            return Ok(match locked_binding {
                Some(binding) => reset_locked(state, binding, now),
                None => reset_unlocked(state, now),
            });
        }
        _ => {}
    }

    if let Err(kind) = ensure_monotonic(&state, now) {
        return Err(TransitionError::new(state, kind));
    }

    let metadata = StateMetadata::from_state(&state);
    if let Some(worker_fence) = metadata.worker_fence {
        return Ok(transition_fenced(
            state.state,
            event,
            now,
            metadata,
            worker_fence,
        ));
    }
    let next = match state.state {
        StateData::Unlocked => Ok(inert(StateData::Unlocked, now, metadata)),
        StateData::LockedUnarmed { binding } => {
            Ok(transition_unarmed(binding, event, now, metadata))
        }
        StateData::LockedArmed {
            binding,
            cooldown_until,
        } => transition_armed(binding, cooldown_until, event, now, metadata),
        StateData::Challenging { request } => transition_challenging(request, event, now, metadata),
        StateData::Cancelling {
            request,
            cooldown_until,
        } => Ok(transition_cancelling(
            request,
            cooldown_until,
            event,
            now,
            metadata,
        )),
        StateData::PermitReady { permit } => {
            Ok(transition_permit_ready(permit, event, now, metadata))
        }
        StateData::Unlocking { binding } => {
            Ok(inert(StateData::Unlocking { binding }, now, metadata))
        }
    };

    match next {
        Ok(success) => Ok(success),
        Err(kind) => Err(TransitionError::new(state, kind)),
    }
}

fn ensure_monotonic(state: &UnlockState, now: MonoMillis) -> Result<(), TransitionErrorKind> {
    if let Some(previous) = state.last_observed_at
        && now < previous
    {
        return Err(TransitionErrorKind::NonMonotonicTime {
            previous,
            current: now,
        });
    }
    Ok(())
}

fn transition_fenced(
    outward_state: StateData,
    event: Event,
    now: MonoMillis,
    mut metadata: StateMetadata,
    worker_fence: WorkerFence,
) -> (UnlockState, Vec<Effect>) {
    if matches!(
        event,
        Event::ChallengeTerminated {
            binding,
            challenge_id,
        } | Event::ChallengeFailed {
            binding,
            challenge_id,
        } | Event::ChallengeTimedOut {
            binding,
            challenge_id,
        } if binding == worker_fence.binding && challenge_id == worker_fence.challenge_id
    ) {
        metadata.worker_fence = None;
    }
    state(outward_state, now, metadata)
}

fn transition_unarmed(
    current: SessionBinding,
    event: Event,
    now: MonoMillis,
    metadata: StateMetadata,
) -> (UnlockState, Vec<Effect>) {
    match event {
        Event::FarStable { binding } | Event::ReliableDisconnect { binding }
            if binding == current =>
        {
            state(
                StateData::LockedArmed {
                    binding: current,
                    cooldown_until: None,
                },
                now,
                metadata,
            )
        }
        _ => inert(StateData::LockedUnarmed { binding: current }, now, metadata),
    }
}

fn transition_armed(
    current: SessionBinding,
    cooldown_until: Option<MonoMillis>,
    event: Event,
    now: MonoMillis,
    mut metadata: StateMetadata,
) -> Result<(UnlockState, Vec<Effect>), TransitionErrorKind> {
    match event {
        Event::NearStable { binding }
            if binding == current && cooldown_elapsed(cooldown_until, now) =>
        {
            let next_id = metadata
                .last_challenge_id
                .checked_add(1)
                .ok_or(TransitionErrorKind::ChallengeIdExhausted)?;
            metadata.last_challenge_id = next_id;
            let challenge_id = ChallengeId::new(next_id);
            let deadline = checked_deadline(
                now,
                metadata.timing_policy.challenge_ttl_ms,
                TimingOperation::ChallengeDeadline,
            )?;
            let request = ChallengeRecord::new(current, challenge_id, deadline);
            Ok(with_effect(
                StateData::Challenging { request },
                now,
                Effect::StartChallenge(request.command()),
                metadata,
            ))
        }
        _ => Ok(inert(
            StateData::LockedArmed {
                binding: current,
                cooldown_until,
            },
            now,
            metadata,
        )),
    }
}

fn transition_challenging(
    request: ChallengeRecord,
    event: Event,
    now: MonoMillis,
    metadata: StateMetadata,
) -> Result<(UnlockState, Vec<Effect>), TransitionErrorKind> {
    if now >= request.deadline {
        if matching_challenge_termination(&event, request) {
            return finish_challenge_with_cooldown(request, now, metadata);
        }
        let cooldown_until = checked_deadline(
            now,
            metadata.timing_policy.cooldown_ms,
            TimingOperation::CooldownDeadline,
        )?;
        return Ok(with_effect(
            StateData::Cancelling {
                request,
                cooldown_until: Some(cooldown_until),
            },
            now,
            Effect::CancelChallenge(request.command()),
            metadata,
        ));
    }

    match event {
        Event::ChallengeVerified(proof)
            if proof.binding == request.binding && proof.challenge_id == request.challenge_id =>
        {
            let expires_at = checked_deadline(
                now,
                metadata.timing_policy.permit_ttl_ms,
                TimingOperation::PermitExpiry,
            )?;
            let permit = PermitRecord {
                binding: request.binding,
                challenge_id: request.challenge_id,
                expires_at,
            };
            Ok(with_effect(
                StateData::PermitReady { permit },
                now,
                Effect::CreatePermit(permit.command()),
                metadata,
            ))
        }
        Event::ChallengeFailed {
            binding,
            challenge_id,
        }
        | Event::ChallengeTimedOut {
            binding,
            challenge_id,
        }
        | Event::ChallengeTerminated {
            binding,
            challenge_id,
        } if binding == request.binding && challenge_id == request.challenge_id => {
            finish_challenge_with_cooldown(request, now, metadata)
        }
        Event::FarStable { binding } | Event::ReliableDisconnect { binding }
            if binding == request.binding =>
        {
            Ok(with_effect(
                StateData::Cancelling {
                    request,
                    cooldown_until: None,
                },
                now,
                Effect::CancelChallenge(request.command()),
                metadata,
            ))
        }
        _ => Ok(inert(StateData::Challenging { request }, now, metadata)),
    }
}

fn matching_challenge_termination(event: &Event, request: ChallengeRecord) -> bool {
    match event {
        Event::ChallengeFailed {
            binding,
            challenge_id,
        }
        | Event::ChallengeTimedOut {
            binding,
            challenge_id,
        }
        | Event::ChallengeTerminated {
            binding,
            challenge_id,
        } => *binding == request.binding && *challenge_id == request.challenge_id,
        _ => false,
    }
}

fn finish_challenge_with_cooldown(
    request: ChallengeRecord,
    now: MonoMillis,
    metadata: StateMetadata,
) -> Result<(UnlockState, Vec<Effect>), TransitionErrorKind> {
    let cooldown_until = checked_deadline(
        now,
        metadata.timing_policy.cooldown_ms,
        TimingOperation::CooldownDeadline,
    )?;
    Ok(state(
        StateData::LockedArmed {
            binding: request.binding,
            cooldown_until: Some(cooldown_until),
        },
        now,
        metadata,
    ))
}

fn transition_cancelling(
    request: ChallengeRecord,
    cooldown_until: Option<MonoMillis>,
    event: Event,
    now: MonoMillis,
    metadata: StateMetadata,
) -> (UnlockState, Vec<Effect>) {
    match event {
        Event::ChallengeTerminated {
            binding,
            challenge_id,
        }
        | Event::ChallengeFailed {
            binding,
            challenge_id,
        }
        | Event::ChallengeTimedOut {
            binding,
            challenge_id,
        } if binding == request.binding && challenge_id == request.challenge_id => state(
            StateData::LockedArmed {
                binding: request.binding,
                cooldown_until,
            },
            now,
            metadata,
        ),
        _ => inert(
            StateData::Cancelling {
                request,
                cooldown_until,
            },
            now,
            metadata,
        ),
    }
}

fn transition_permit_ready(
    permit: PermitRecord,
    event: Event,
    now: MonoMillis,
    metadata: StateMetadata,
) -> (UnlockState, Vec<Effect>) {
    if now >= permit.expires_at {
        return with_effect(
            StateData::LockedArmed {
                binding: permit.binding,
                cooldown_until: None,
            },
            now,
            Effect::ClearPermit,
            metadata,
        );
    }

    match event {
        Event::PermitConsumed {
            binding,
            challenge_id,
        } if binding == permit.binding && challenge_id == permit.challenge_id => state(
            StateData::Unlocking {
                binding: permit.binding,
            },
            now,
            metadata,
        ),
        Event::PermitExpired {
            binding,
            challenge_id,
        } if binding == permit.binding && challenge_id == permit.challenge_id => with_effect(
            StateData::LockedArmed {
                binding: permit.binding,
                cooldown_until: None,
            },
            now,
            Effect::ClearPermit,
            metadata,
        ),
        _ => inert(StateData::PermitReady { permit }, now, metadata),
    }
}

fn cooldown_elapsed(cooldown_until: Option<MonoMillis>, now: MonoMillis) -> bool {
    cooldown_until.is_none_or(|deadline| now >= deadline)
}

fn checked_deadline(
    now: MonoMillis,
    duration_ms: u64,
    operation: TimingOperation,
) -> Result<MonoMillis, TransitionErrorKind> {
    now.get()
        .checked_add(duration_ms)
        .map(MonoMillis::new)
        .ok_or(TransitionErrorKind::TimeOverflow {
            operation,
            now,
            duration_ms,
        })
}

fn reset_locked(
    previous: UnlockState,
    binding: SessionBinding,
    now: MonoMillis,
) -> (UnlockState, Vec<Effect>) {
    let current_binding = previous.binding();
    let stale_binding = previous.authoritative_binding.is_some_and(|highest| {
        binding.lock_epoch < highest.lock_epoch
            || (binding.lock_epoch == highest.lock_epoch && binding != highest)
    });
    let (mut metadata, effects) = reset_metadata_and_effects(&previous);
    if stale_binding {
        return match current_binding {
            Some(current) => with_effects(
                StateData::LockedUnarmed { binding: current },
                now,
                effects,
                metadata,
            ),
            None => with_effects(StateData::Unlocked, now, effects, metadata),
        };
    }

    metadata.authoritative_binding = Some(binding);
    with_effects(StateData::LockedUnarmed { binding }, now, effects, metadata)
}

fn reset_unlocked(previous: UnlockState, now: MonoMillis) -> (UnlockState, Vec<Effect>) {
    let (metadata, effects) = reset_metadata_and_effects(&previous);
    with_effects(StateData::Unlocked, now, effects, metadata)
}

fn reset_metadata_and_effects(previous: &UnlockState) -> (StateMetadata, Vec<Effect>) {
    let mut metadata = StateMetadata::from_state(previous);
    if metadata.worker_fence.is_none() {
        metadata.worker_fence = match &previous.state {
            StateData::Challenging { request } | StateData::Cancelling { request, .. } => {
                Some(WorkerFence {
                    binding: request.binding,
                    challenge_id: request.challenge_id,
                })
            }
            _ => None,
        };
    }

    let effects = if metadata.worker_fence.is_some() {
        vec![Effect::AbortAllChallenges, Effect::ClearPermit]
    } else {
        vec![Effect::ClearPermit]
    };
    (metadata, effects)
}

fn inert(
    state_data: StateData,
    now: MonoMillis,
    metadata: StateMetadata,
) -> (UnlockState, Vec<Effect>) {
    state(state_data, now, metadata)
}

fn state(
    state_data: StateData,
    now: MonoMillis,
    metadata: StateMetadata,
) -> (UnlockState, Vec<Effect>) {
    (
        UnlockState {
            state: state_data,
            last_observed_at: Some(now),
            authoritative_binding: metadata.authoritative_binding,
            last_challenge_id: metadata.last_challenge_id,
            timing_policy: metadata.timing_policy,
            worker_fence: metadata.worker_fence,
        },
        Vec::new(),
    )
}

fn with_effect(
    state_data: StateData,
    now: MonoMillis,
    effect: Effect,
    metadata: StateMetadata,
) -> (UnlockState, Vec<Effect>) {
    with_effect_and_high_water(state_data, now, effect, metadata)
}

fn with_effect_and_high_water(
    state_data: StateData,
    now: MonoMillis,
    effect: Effect,
    metadata: StateMetadata,
) -> (UnlockState, Vec<Effect>) {
    with_effects(state_data, now, vec![effect], metadata)
}

fn with_effects(
    state_data: StateData,
    now: MonoMillis,
    effects: Vec<Effect>,
    metadata: StateMetadata,
) -> (UnlockState, Vec<Effect>) {
    (
        UnlockState {
            state: state_data,
            last_observed_at: Some(now),
            authoritative_binding: metadata.authoritative_binding,
            last_challenge_id: metadata.last_challenge_id,
            timing_policy: metadata.timing_policy,
            worker_fence: metadata.worker_fence,
        },
        effects,
    )
}

#[derive(Debug, Clone, Copy)]
struct StateMetadata {
    authoritative_binding: Option<SessionBinding>,
    last_challenge_id: u64,
    timing_policy: TimingPolicy,
    worker_fence: Option<WorkerFence>,
}

impl StateMetadata {
    const fn from_state(state: &UnlockState) -> Self {
        Self {
            authoritative_binding: state.authoritative_binding,
            last_challenge_id: state.last_challenge_id,
            timing_policy: state.timing_policy,
            worker_fence: state.worker_fence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(value: u64) -> MonoMillis {
        MonoMillis::new(value)
    }

    fn binding(epoch: u64, audit_session: u32, uid: u32) -> SessionBinding {
        SessionBinding::new(
            LockEpoch::new(epoch),
            AuditSessionId::new(audit_session),
            ConsoleUid::new(uid),
        )
    }

    fn policy() -> TimingPolicy {
        TimingPolicy::new(20, 3, 6).unwrap()
    }

    fn challenging(current: SessionBinding) -> UnlockState {
        let (locked, _) = transition(
            UnlockState::unlocked(policy()),
            Event::SessionLocked { binding: current },
            time(1),
        )
        .unwrap();
        let (armed, _) =
            transition(locked, Event::FarStable { binding: current }, time(2)).unwrap();
        transition(armed, Event::NearStable { binding: current }, time(3))
            .unwrap()
            .0
    }

    fn permit_ready(current: SessionBinding) -> UnlockState {
        let state = challenging(current);
        let challenge_id = state.challenge_id().unwrap();
        transition(
            state,
            Event::ChallengeVerified(ChallengeVerified::new(current, challenge_id)),
            time(4),
        )
        .unwrap()
        .0
    }

    #[test]
    fn exact_verified_challenge_creates_a_bound_permit() {
        let current = binding(7, 41, 501);
        let state = challenging(current);
        let challenge_id = state.challenge_id().unwrap();

        let (next, effects) = transition(
            state,
            Event::ChallengeVerified(ChallengeVerified::new(current, challenge_id)),
            time(4),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::PermitReady);
        assert_eq!(next.binding(), Some(current));
        assert_eq!(next.permit_expires_at(), Some(time(7)));
        assert_eq!(effects.len(), 1);
        let Effect::CreatePermit(permit) = &effects[0] else {
            panic!("expected a create-permit effect");
        };
        assert_eq!(permit.binding(), current);
        assert_eq!(permit.challenge_id(), challenge_id);
        assert_eq!(permit.expires_at(), time(7));
    }

    #[test]
    fn stale_epoch_uid_and_audit_proofs_never_create_permits() {
        let current = binding(7, 41, 501);
        let stale_bindings = [
            binding(6, 41, 501),
            binding(7, 41, 502),
            binding(7, 42, 501),
        ];

        for stale in stale_bindings {
            let state = challenging(current);
            let challenge_id = state.challenge_id().unwrap();
            let (next, effects) = transition(
                state,
                Event::ChallengeVerified(ChallengeVerified::new(stale, challenge_id)),
                time(4),
            )
            .unwrap();
            assert_eq!(next.phase(), UnlockPhase::Challenging);
            assert!(!effects.iter().any(Effect::is_create_permit));
        }
    }

    #[test]
    fn stale_challenge_id_never_creates_a_permit() {
        let current = binding(7, 41, 501);

        let (next, effects) = transition(
            challenging(current),
            Event::ChallengeVerified(ChallengeVerified::new(current, ChallengeId::new(2))),
            time(4),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::Challenging);
        assert!(!effects.iter().any(Effect::is_create_permit));
    }

    #[test]
    fn proof_at_or_after_challenge_deadline_never_creates_a_permit() {
        let current = binding(7, 41, 501);

        for now in [23, 24] {
            let state = challenging(current);
            let challenge_id = state.challenge_id().unwrap();
            let (next, effects) = transition(
                state,
                Event::ChallengeVerified(ChallengeVerified::new(current, challenge_id)),
                time(now),
            )
            .unwrap();
            assert_eq!(next.phase(), UnlockPhase::Cancelling);
            assert!(effects.iter().any(Effect::is_cancel_challenge));
            assert!(!effects.iter().any(Effect::is_create_permit));
        }
    }

    #[test]
    fn consuming_a_live_permit_enters_unlocking() {
        let current = binding(7, 41, 501);
        let state = permit_ready(current);
        let challenge_id = state.challenge_id().unwrap();

        let (next, effects) = transition(
            state,
            Event::PermitConsumed {
                binding: current,
                challenge_id,
            },
            time(5),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::Unlocking);
        assert!(effects.is_empty());
    }

    #[test]
    fn expiring_a_permit_returns_to_armed_and_clears_it() {
        let current = binding(7, 41, 501);
        let state = permit_ready(current);
        let challenge_id = state.challenge_id().unwrap();

        let (next, effects) = transition(
            state,
            Event::PermitExpired {
                binding: current,
                challenge_id,
            },
            time(7),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::LockedArmed);
        assert_eq!(effects, vec![Effect::ClearPermit]);
    }

    #[test]
    fn permit_consumption_at_expiry_fails_closed() {
        let current = binding(7, 41, 501);
        let state = permit_ready(current);
        let challenge_id = state.challenge_id().unwrap();

        let (next, effects) = transition(
            state,
            Event::PermitConsumed {
                binding: current,
                challenge_id,
            },
            time(7),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::LockedArmed);
        assert_eq!(effects, vec![Effect::ClearPermit]);
    }

    #[test]
    fn restart_cannot_restore_permit_ready() {
        let current = binding(7, 41, 501);

        let (next, effects) = transition(
            permit_ready(current),
            Event::ServiceRestarted {
                locked_binding: Some(current),
            },
            time(5),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(next.permit_expires_at(), None);
        assert_eq!(effects, vec![Effect::ClearPermit]);
    }

    #[test]
    fn authoritative_resets_rebase_time_even_from_permit_ready() {
        let current = binding(7, 41, 501);
        let replacement = binding(8, 42, 502);
        let cases = [
            (Event::SessionUnlocked, UnlockPhase::Unlocked, None),
            (Event::Logout, UnlockPhase::Unlocked, None),
            (
                Event::FastUserSwitch {
                    locked_binding: Some(replacement),
                },
                UnlockPhase::LockedUnarmed,
                Some(replacement),
            ),
            (
                Event::ServiceRestarted {
                    locked_binding: Some(current),
                },
                UnlockPhase::LockedUnarmed,
                Some(current),
            ),
        ];

        for (event, expected_phase, expected_binding) in cases {
            let (next, effects) = transition(permit_ready(current), event, time(2)).unwrap();
            assert_eq!(next.phase(), expected_phase);
            assert_eq!(next.binding(), expected_binding);
            assert_eq!(next.last_observed_at(), Some(time(2)));
            assert_eq!(next.permit_expires_at(), None);
            assert_eq!(effects, vec![Effect::ClearPermit]);
        }
    }

    #[test]
    fn stale_lifecycle_epoch_clears_permit_without_reviving_stale_proof() {
        let current = binding(8, 42, 502);
        let stale = binding(7, 41, 501);

        let (reset, effects) = transition(
            permit_ready(current),
            Event::ServiceRestarted {
                locked_binding: Some(stale),
            },
            time(5),
        )
        .unwrap();
        assert_eq!(reset.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(reset.binding(), Some(current));
        assert_eq!(effects, vec![Effect::ClearPermit]);

        let (next, effects) = transition(
            reset,
            Event::ChallengeVerified(ChallengeVerified::new(stale, ChallengeId::new(1))),
            time(6),
        )
        .unwrap();
        assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(next.binding(), Some(current));
        assert!(!effects.iter().any(Effect::is_create_permit));
    }

    #[test]
    fn verified_proof_is_inert_while_its_challenge_is_cancelling() {
        let current = binding(7, 41, 501);
        let state = challenging(current);
        let challenge_id = state.challenge_id().unwrap();
        let (cancelling, effects) =
            transition(state, Event::FarStable { binding: current }, time(4)).unwrap();
        assert_eq!(cancelling.phase(), UnlockPhase::Cancelling);
        assert!(effects.iter().any(Effect::is_cancel_challenge));

        let (next, effects) = transition(
            cancelling,
            Event::ChallengeVerified(ChallengeVerified::new(current, challenge_id)),
            time(5),
        )
        .unwrap();
        assert_eq!(next.phase(), UnlockPhase::Cancelling);
        assert!(!effects.iter().any(Effect::is_create_permit));
    }

    #[test]
    fn challenge_id_overflow_is_rejected_without_starting_work() {
        let current = binding(7, 41, 501);
        let mut state = transition(
            transition(
                UnlockState::unlocked(TimingPolicy::new(20, 3, 5).unwrap()),
                Event::SessionLocked { binding: current },
                time(1),
            )
            .unwrap()
            .0,
            Event::FarStable { binding: current },
            time(2),
        )
        .unwrap()
        .0;
        state.last_challenge_id = u64::MAX;

        let error = transition(state, Event::NearStable { binding: current }, time(3)).unwrap_err();
        assert_eq!(error.kind(), TransitionErrorKind::ChallengeIdExhausted);
        let recovered = error.into_state();
        assert_eq!(recovered.phase(), UnlockPhase::LockedArmed);
        assert_eq!(recovered.binding(), Some(current));
        assert_eq!(recovered.authoritative_binding, Some(current));
        assert_eq!(recovered.last_challenge_id, u64::MAX);
        assert_eq!(recovered.last_observed_at, Some(time(2)));

        let error =
            transition(recovered, Event::NearStable { binding: current }, time(3)).unwrap_err();
        assert_eq!(error.kind(), TransitionErrorKind::ChallengeIdExhausted);
        assert_eq!(error.into_state().last_challenge_id, u64::MAX);
    }

    #[test]
    fn permit_expiry_arithmetic_is_checked() {
        let current = binding(7, 41, 501);
        let mut state = challenging(current);
        let challenge_id = state.challenge_id().unwrap();
        state.state = StateData::Challenging {
            request: ChallengeRecord::new(current, challenge_id, time(u64::MAX)),
        };
        state.last_observed_at = Some(time(u64::MAX - 3));
        let now = time(u64::MAX - 2);

        let error = transition(
            state,
            Event::ChallengeVerified(ChallengeVerified::new(current, challenge_id)),
            now,
        )
        .unwrap_err();
        assert_eq!(
            error.kind(),
            TransitionErrorKind::TimeOverflow {
                operation: TimingOperation::PermitExpiry,
                now,
                duration_ms: policy().permit_ttl_ms(),
            }
        );
        let recovered = error.into_state();
        assert_eq!(recovered.phase(), UnlockPhase::Challenging);
        assert_eq!(recovered.binding(), Some(current));
        assert_eq!(recovered.authoritative_binding, Some(current));
        assert_eq!(recovered.challenge_id(), Some(challenge_id));
        assert_eq!(recovered.last_challenge_id, challenge_id.get());
        assert_eq!(recovered.last_observed_at, Some(time(u64::MAX - 3)));

        let (permit_ready, effects) = transition(
            recovered,
            Event::ChallengeVerified(ChallengeVerified::new(current, challenge_id)),
            time(u64::MAX - 3),
        )
        .unwrap();
        assert_eq!(permit_ready.phase(), UnlockPhase::PermitReady);
        assert_eq!(permit_ready.permit_expires_at(), Some(time(u64::MAX)));
        assert_eq!(effects.len(), 1);
        assert!(effects[0].is_create_permit());
    }

    #[test]
    fn proof_at_the_challenge_deadline_cancels_instead_of_creating_a_permit() {
        let current = binding(7, 41, 501);
        let state = challenging(current);
        let challenge_id = state.challenge_id().unwrap();

        let (next, effects) = transition(
            state,
            Event::ChallengeVerified(ChallengeVerified::new(current, challenge_id)),
            time(23),
        )
        .unwrap();
        assert_eq!(next.phase(), UnlockPhase::Cancelling);
        assert!(effects.iter().any(Effect::is_cancel_challenge));
        assert!(!effects.iter().any(Effect::is_create_permit));
    }

    #[test]
    fn a_late_old_proof_cannot_match_a_newer_attempt() {
        let current = binding(7, 41, 501);
        let first = challenging(current);
        let first_id = first.challenge_id().unwrap();
        let (cancelling, _) =
            transition(first, Event::FarStable { binding: current }, time(4)).unwrap();
        let (armed, _) = transition(
            cancelling,
            Event::ChallengeTerminated {
                binding: current,
                challenge_id: first_id,
            },
            time(5),
        )
        .unwrap();
        let (second, _) =
            transition(armed, Event::NearStable { binding: current }, time(6)).unwrap();
        let second_id = second.challenge_id().unwrap();
        assert_ne!(second_id, first_id);

        let (next, effects) = transition(
            second,
            Event::ChallengeVerified(ChallengeVerified::new(current, first_id)),
            time(7),
        )
        .unwrap();
        assert_eq!(next.phase(), UnlockPhase::Challenging);
        assert_eq!(next.challenge_id(), Some(second_id));
        assert!(!effects.iter().any(Effect::is_create_permit));
    }

    #[test]
    fn permit_naturally_expires_at_or_after_its_policy_deadline() {
        let current = binding(7, 41, 501);

        let (before, effects) = transition(permit_ready(current), Event::Tick, time(6)).unwrap();
        assert_eq!(before.phase(), UnlockPhase::PermitReady);
        assert!(effects.is_empty());

        for now in [7, 8] {
            let (next, effects) =
                transition(permit_ready(current), Event::Tick, time(now)).unwrap();
            assert_eq!(next.phase(), UnlockPhase::LockedArmed);
            assert_eq!(effects, vec![Effect::ClearPermit]);
        }
    }

    #[test]
    fn proof_after_global_reset_is_inert_until_the_worker_fence_is_retired() {
        let current = binding(7, 41, 501);
        let active = challenging(current);
        let challenge_id = active.challenge_id().unwrap();
        let (reset, effects) = transition(active, Event::SessionUnlocked, time(4)).unwrap();
        assert_eq!(reset.phase(), UnlockPhase::Unlocked);
        assert_eq!(
            effects,
            vec![Effect::AbortAllChallenges, Effect::ClearPermit]
        );

        let (after_proof, effects) = transition(
            reset,
            Event::ChallengeVerified(ChallengeVerified::new(current, challenge_id)),
            time(5),
        )
        .unwrap();
        assert_eq!(after_proof.phase(), UnlockPhase::Unlocked);
        assert!(!effects.iter().any(Effect::is_create_permit));

        let (retired, effects) = transition(
            after_proof,
            Event::ChallengeTerminated {
                binding: current,
                challenge_id,
            },
            time(6),
        )
        .unwrap();
        assert_eq!(retired.phase(), UnlockPhase::Unlocked);
        assert!(effects.is_empty());
    }
}
