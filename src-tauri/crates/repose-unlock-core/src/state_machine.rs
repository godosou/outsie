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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChallengeId(u64);

impl ChallengeId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChallengeRequest {
    binding: SessionBinding,
    challenge_id: ChallengeId,
    deadline: MonoMillis,
}

impl ChallengeRequest {
    #[must_use]
    pub const fn new(
        binding: SessionBinding,
        challenge_id: ChallengeId,
        deadline: MonoMillis,
    ) -> Self {
        Self {
            binding,
            challenge_id,
            deadline,
        }
    }

    #[must_use]
    pub const fn binding(self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn challenge_id(self) -> ChallengeId {
        self.challenge_id
    }

    #[must_use]
    pub const fn deadline(self) -> MonoMillis {
        self.deadline
    }
}

/// Evidence that the protocol layer has authenticated a response for one challenge.
///
/// The fields and constructor are intentionally not public. External callers can
/// submit this type through [`Event::ChallengeVerified`] after a verifier in this
/// crate creates it, but cannot manufacture verified evidence through the safe API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChallengeVerified {
    binding: SessionBinding,
    challenge_id: ChallengeId,
    permit_expires_at: MonoMillis,
}

impl ChallengeVerified {
    #[allow(dead_code)]
    pub(crate) const fn new(
        binding: SessionBinding,
        challenge_id: ChallengeId,
        permit_expires_at: MonoMillis,
    ) -> Self {
        Self {
            binding,
            challenge_id,
            permit_expires_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permit {
    binding: SessionBinding,
    challenge_id: ChallengeId,
    expires_at: MonoMillis,
}

impl Permit {
    #[must_use]
    pub const fn binding(self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn challenge_id(self) -> ChallengeId {
        self.challenge_id
    }

    #[must_use]
    pub const fn expires_at(self) -> MonoMillis {
        self.expires_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    StartChallenge(ChallengeRequest),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        challenge_id: ChallengeId,
        deadline: MonoMillis,
    },
    ChallengeVerified(ChallengeVerified),
    ChallengeFailed {
        binding: SessionBinding,
        challenge_id: ChallengeId,
        cooldown_until: MonoMillis,
    },
    ChallengeTimedOut {
        binding: SessionBinding,
        challenge_id: ChallengeId,
        cooldown_until: MonoMillis,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockPhase {
    Unlocked,
    LockedUnarmed,
    LockedArmed,
    Challenging,
    PermitReady,
    Unlocking,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlockState {
    state: StateData,
    last_observed_at: Option<MonoMillis>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        request: ChallengeRequest,
    },
    PermitReady {
        permit: Permit,
    },
    Unlocking {
        binding: SessionBinding,
    },
}

impl UnlockState {
    #[must_use]
    pub const fn unlocked() -> Self {
        Self {
            state: StateData::Unlocked,
            last_observed_at: None,
        }
    }

    #[must_use]
    pub const fn phase(&self) -> UnlockPhase {
        match self.state {
            StateData::Unlocked => UnlockPhase::Unlocked,
            StateData::LockedUnarmed { .. } => UnlockPhase::LockedUnarmed,
            StateData::LockedArmed { .. } => UnlockPhase::LockedArmed,
            StateData::Challenging { .. } => UnlockPhase::Challenging,
            StateData::PermitReady { .. } => UnlockPhase::PermitReady,
            StateData::Unlocking { .. } => UnlockPhase::Unlocking,
        }
    }

    #[must_use]
    pub const fn binding(&self) -> Option<SessionBinding> {
        match self.state {
            StateData::Unlocked => None,
            StateData::LockedUnarmed { binding }
            | StateData::LockedArmed { binding, .. }
            | StateData::Unlocking { binding } => Some(binding),
            StateData::Challenging { request } => Some(request.binding),
            StateData::PermitReady { permit } => Some(permit.binding),
        }
    }

    #[must_use]
    pub const fn challenge_id(&self) -> Option<ChallengeId> {
        match self.state {
            StateData::Challenging { request } => Some(request.challenge_id),
            StateData::PermitReady { permit } => Some(permit.challenge_id),
            _ => None,
        }
    }

    #[must_use]
    pub const fn cooldown_until(&self) -> Option<MonoMillis> {
        match self.state {
            StateData::LockedArmed { cooldown_until, .. } => cooldown_until,
            _ => None,
        }
    }

    #[must_use]
    pub const fn permit_expires_at(&self) -> Option<MonoMillis> {
        match self.state {
            StateData::PermitReady { permit } => Some(permit.expires_at),
            _ => None,
        }
    }

    #[must_use]
    pub const fn last_observed_at(&self) -> Option<MonoMillis> {
        self.last_observed_at
    }
}

impl Default for UnlockState {
    fn default() -> Self {
        Self::unlocked()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    NonMonotonicTime {
        previous: MonoMillis,
        current: MonoMillis,
    },
    DeadlineNotInFuture {
        now: MonoMillis,
        deadline: MonoMillis,
    },
}

impl Display for TransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonMonotonicTime { previous, current } => write!(
                formatter,
                "monotonic time moved backwards from {} to {}",
                previous.get(),
                current.get()
            ),
            Self::DeadlineNotInFuture { now, deadline } => write!(
                formatter,
                "deadline {} must be later than current monotonic time {}",
                deadline.get(),
                now.get()
            ),
        }
    }
}

impl Error for TransitionError {}

pub fn transition(
    state: UnlockState,
    event: Event,
    now: MonoMillis,
) -> Result<(UnlockState, Vec<Effect>), TransitionError> {
    ensure_monotonic(&state, now)?;

    match event {
        Event::SessionLocked { binding } => {
            return Ok(reset_locked(binding, now));
        }
        Event::SessionUnlocked | Event::Logout => {
            return Ok(reset_unlocked(now));
        }
        Event::FastUserSwitch { locked_binding } | Event::ServiceRestarted { locked_binding } => {
            return Ok(match locked_binding {
                Some(binding) => reset_locked(binding, now),
                None => reset_unlocked(now),
            });
        }
        _ => {}
    }

    let next = match state.state {
        StateData::Unlocked => inert(StateData::Unlocked, now),
        StateData::LockedUnarmed { binding } => transition_unarmed(binding, event, now),
        StateData::LockedArmed {
            binding,
            cooldown_until,
        } => transition_armed(binding, cooldown_until, event, now)?,
        StateData::Challenging { request } => transition_challenging(request, event, now)?,
        StateData::PermitReady { permit } => transition_permit_ready(permit, event, now),
        StateData::Unlocking { binding } => inert(StateData::Unlocking { binding }, now),
    };

    Ok(next)
}

fn ensure_monotonic(state: &UnlockState, now: MonoMillis) -> Result<(), TransitionError> {
    if let Some(previous) = state.last_observed_at
        && now < previous
    {
        return Err(TransitionError::NonMonotonicTime {
            previous,
            current: now,
        });
    }
    Ok(())
}

fn transition_unarmed(
    current: SessionBinding,
    event: Event,
    now: MonoMillis,
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
            )
        }
        _ => inert(StateData::LockedUnarmed { binding: current }, now),
    }
}

fn transition_armed(
    current: SessionBinding,
    cooldown_until: Option<MonoMillis>,
    event: Event,
    now: MonoMillis,
) -> Result<(UnlockState, Vec<Effect>), TransitionError> {
    match event {
        Event::NearStable {
            binding,
            challenge_id,
            deadline,
        } if binding == current && cooldown_elapsed(cooldown_until, now) => {
            if deadline <= now {
                return Err(TransitionError::DeadlineNotInFuture { now, deadline });
            }
            let request = ChallengeRequest::new(current, challenge_id, deadline);
            Ok(with_effect(
                StateData::Challenging { request },
                now,
                Effect::StartChallenge(request),
            ))
        }
        _ => Ok(inert(
            StateData::LockedArmed {
                binding: current,
                cooldown_until,
            },
            now,
        )),
    }
}

fn transition_challenging(
    request: ChallengeRequest,
    event: Event,
    now: MonoMillis,
) -> Result<(UnlockState, Vec<Effect>), TransitionError> {
    match event {
        Event::ChallengeVerified(proof)
            if proof.binding == request.binding
                && proof.challenge_id == request.challenge_id
                && now < request.deadline =>
        {
            if proof.permit_expires_at <= now {
                return Err(TransitionError::DeadlineNotInFuture {
                    now,
                    deadline: proof.permit_expires_at,
                });
            }
            let permit = Permit {
                binding: request.binding,
                challenge_id: request.challenge_id,
                expires_at: proof.permit_expires_at,
            };
            Ok(with_effect(
                StateData::PermitReady { permit },
                now,
                Effect::CreatePermit(permit),
            ))
        }
        Event::ChallengeFailed {
            binding,
            challenge_id,
            cooldown_until,
        }
        | Event::ChallengeTimedOut {
            binding,
            challenge_id,
            cooldown_until,
        } if binding == request.binding && challenge_id == request.challenge_id => Ok(state(
            StateData::LockedArmed {
                binding: request.binding,
                cooldown_until: Some(cooldown_until),
            },
            now,
        )),
        Event::FarStable { binding } | Event::ReliableDisconnect { binding }
            if binding == request.binding =>
        {
            Ok(state(
                StateData::LockedArmed {
                    binding: request.binding,
                    cooldown_until: None,
                },
                now,
            ))
        }
        _ => Ok(inert(StateData::Challenging { request }, now)),
    }
}

fn transition_permit_ready(
    permit: Permit,
    event: Event,
    now: MonoMillis,
) -> (UnlockState, Vec<Effect>) {
    if now >= permit.expires_at {
        return with_effect(
            StateData::LockedArmed {
                binding: permit.binding,
                cooldown_until: None,
            },
            now,
            Effect::ClearPermit,
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
        ),
        _ => inert(StateData::PermitReady { permit }, now),
    }
}

fn cooldown_elapsed(cooldown_until: Option<MonoMillis>, now: MonoMillis) -> bool {
    cooldown_until.is_none_or(|deadline| now >= deadline)
}

fn reset_locked(binding: SessionBinding, now: MonoMillis) -> (UnlockState, Vec<Effect>) {
    with_effect(
        StateData::LockedUnarmed { binding },
        now,
        Effect::ClearPermit,
    )
}

fn reset_unlocked(now: MonoMillis) -> (UnlockState, Vec<Effect>) {
    with_effect(StateData::Unlocked, now, Effect::ClearPermit)
}

fn inert(state_data: StateData, now: MonoMillis) -> (UnlockState, Vec<Effect>) {
    state(state_data, now)
}

fn state(state_data: StateData, now: MonoMillis) -> (UnlockState, Vec<Effect>) {
    (
        UnlockState {
            state: state_data,
            last_observed_at: Some(now),
        },
        Vec::new(),
    )
}

fn with_effect(
    state_data: StateData,
    now: MonoMillis,
    effect: Effect,
) -> (UnlockState, Vec<Effect>) {
    (
        UnlockState {
            state: state_data,
            last_observed_at: Some(now),
        },
        vec![effect],
    )
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

    fn challenging(current: SessionBinding) -> UnlockState {
        let (locked, _) = transition(
            UnlockState::unlocked(),
            Event::SessionLocked { binding: current },
            time(1),
        )
        .unwrap();
        let (armed, _) =
            transition(locked, Event::FarStable { binding: current }, time(2)).unwrap();
        transition(
            armed,
            Event::NearStable {
                binding: current,
                challenge_id: ChallengeId::new(9),
                deadline: time(20),
            },
            time(3),
        )
        .unwrap()
        .0
    }

    fn permit_ready(current: SessionBinding, expires_at: MonoMillis) -> UnlockState {
        transition(
            challenging(current),
            Event::ChallengeVerified(ChallengeVerified::new(
                current,
                ChallengeId::new(9),
                expires_at,
            )),
            time(4),
        )
        .unwrap()
        .0
    }

    #[test]
    fn exact_verified_challenge_creates_a_bound_permit() {
        let current = binding(7, 41, 501);

        let (next, effects) = transition(
            challenging(current),
            Event::ChallengeVerified(ChallengeVerified::new(
                current,
                ChallengeId::new(9),
                time(12),
            )),
            time(4),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::PermitReady);
        assert_eq!(next.binding(), Some(current));
        assert_eq!(next.permit_expires_at(), Some(time(12)));
        assert_eq!(effects.len(), 1);
        let Effect::CreatePermit(permit) = effects[0] else {
            panic!("expected a create-permit effect");
        };
        assert_eq!(permit.binding(), current);
        assert_eq!(permit.challenge_id(), ChallengeId::new(9));
        assert_eq!(permit.expires_at(), time(12));
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
            let (next, effects) = transition(
                challenging(current),
                Event::ChallengeVerified(ChallengeVerified::new(
                    stale,
                    ChallengeId::new(9),
                    time(12),
                )),
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
            Event::ChallengeVerified(ChallengeVerified::new(
                current,
                ChallengeId::new(8),
                time(12),
            )),
            time(4),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::Challenging);
        assert!(!effects.iter().any(Effect::is_create_permit));
    }

    #[test]
    fn proof_at_or_after_challenge_deadline_never_creates_a_permit() {
        let current = binding(7, 41, 501);

        for now in [20, 21] {
            let (next, effects) = transition(
                challenging(current),
                Event::ChallengeVerified(ChallengeVerified::new(
                    current,
                    ChallengeId::new(9),
                    time(30),
                )),
                time(now),
            )
            .unwrap();
            assert_eq!(next.phase(), UnlockPhase::Challenging);
            assert!(!effects.iter().any(Effect::is_create_permit));
        }
    }

    #[test]
    fn consuming_a_live_permit_enters_unlocking() {
        let current = binding(7, 41, 501);

        let (next, effects) = transition(
            permit_ready(current, time(12)),
            Event::PermitConsumed {
                binding: current,
                challenge_id: ChallengeId::new(9),
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

        let (next, effects) = transition(
            permit_ready(current, time(12)),
            Event::PermitExpired {
                binding: current,
                challenge_id: ChallengeId::new(9),
            },
            time(12),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::LockedArmed);
        assert_eq!(effects, vec![Effect::ClearPermit]);
    }

    #[test]
    fn permit_consumption_at_expiry_fails_closed() {
        let current = binding(7, 41, 501);

        let (next, effects) = transition(
            permit_ready(current, time(12)),
            Event::PermitConsumed {
                binding: current,
                challenge_id: ChallengeId::new(9),
            },
            time(12),
        )
        .unwrap();

        assert_eq!(next.phase(), UnlockPhase::LockedArmed);
        assert_eq!(effects, vec![Effect::ClearPermit]);
    }

    #[test]
    fn restart_cannot_restore_permit_ready() {
        let current = binding(7, 41, 501);

        let (next, effects) = transition(
            permit_ready(current, time(12)),
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
}
