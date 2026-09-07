use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};
use repose_unlock_core::state_machine::{
    ChallengeId, ChallengeRequest, Effect, Event, SessionBinding, TransitionError, UnlockPhase,
    UnlockState, transition,
};

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

fn locked_unarmed(binding: SessionBinding) -> UnlockState {
    transition(
        UnlockState::unlocked(),
        Event::SessionLocked { binding },
        time(1),
    )
    .unwrap()
    .0
}

fn locked_armed(binding: SessionBinding) -> UnlockState {
    transition(
        locked_unarmed(binding),
        Event::FarStable { binding },
        time(2),
    )
    .unwrap()
    .0
}

fn challenging(binding: SessionBinding, id: u64) -> UnlockState {
    transition(
        locked_armed(binding),
        Event::NearStable {
            binding,
            challenge_id: ChallengeId::new(id),
            deadline: time(50),
        },
        time(3),
    )
    .unwrap()
    .0
}

#[test]
fn near_without_observed_departure_does_not_start_a_challenge() {
    let current = binding(7, 41, 501);
    let state = locked_unarmed(current);

    let (next, effects) = transition(
        state,
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(1),
            deadline: time(50),
        },
        time(2),
    )
    .unwrap();

    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert!(!effects.iter().any(Effect::is_start_challenge));
}

#[test]
fn departure_then_return_starts_exactly_one_challenge() {
    let current = binding(7, 41, 501);
    let request = ChallengeRequest::new(current, ChallengeId::new(22), time(50));
    let state = locked_armed(current);

    let (next, effects) = transition(
        state,
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(22),
            deadline: time(50),
        },
        time(3),
    )
    .unwrap();

    assert_eq!(next.phase(), UnlockPhase::Challenging);
    assert_eq!(effects, vec![Effect::StartChallenge(request)]);
}

#[test]
fn reliable_disconnect_also_arms_a_locked_session() {
    let current = binding(7, 41, 501);

    let (state, effects) = transition(
        locked_unarmed(current),
        Event::ReliableDisconnect { binding: current },
        time(2),
    )
    .unwrap();

    assert_eq!(state.phase(), UnlockPhase::LockedArmed);
    assert!(effects.is_empty());
}

#[test]
fn service_restart_while_locked_is_always_unarmed_and_clears_transients() {
    let current = binding(7, 41, 501);
    let state = challenging(current, 22);

    let (next, effects) = transition(
        state,
        Event::ServiceRestarted {
            locked_binding: Some(current),
        },
        time(4),
    )
    .unwrap();

    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(next.binding(), Some(current));
    assert_eq!(next.challenge_id(), None);
    assert_eq!(next.permit_expires_at(), None);
    assert_eq!(effects, vec![Effect::ClearPermit]);
}

#[test]
fn uid_change_resets_a_live_challenge_to_unarmed() {
    let old = binding(7, 41, 501);
    let changed_uid = binding(8, 41, 502);

    let (next, effects) = transition(
        challenging(old, 22),
        Event::SessionLocked {
            binding: changed_uid,
        },
        time(4),
    )
    .unwrap();

    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(next.binding(), Some(changed_uid));
    assert_eq!(effects, vec![Effect::ClearPermit]);
}

#[test]
fn audit_session_change_resets_a_live_challenge_to_unarmed() {
    let old = binding(7, 41, 501);
    let changed_audit_session = binding(8, 42, 501);

    let (next, effects) = transition(
        challenging(old, 22),
        Event::SessionLocked {
            binding: changed_audit_session,
        },
        time(4),
    )
    .unwrap();

    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(next.binding(), Some(changed_audit_session));
    assert_eq!(effects, vec![Effect::ClearPermit]);
}

#[test]
fn stale_epoch_proximity_and_failure_events_are_inert() {
    let current = binding(8, 41, 501);
    let stale = binding(7, 41, 501);
    let state = locked_unarmed(current);

    let stale_events = [
        Event::FarStable { binding: stale },
        Event::ReliableDisconnect { binding: stale },
        Event::NearStable {
            binding: stale,
            challenge_id: ChallengeId::new(1),
            deadline: time(50),
        },
        Event::ChallengeFailed {
            binding: stale,
            challenge_id: ChallengeId::new(1),
            cooldown_until: time(50),
        },
        Event::ChallengeTimedOut {
            binding: stale,
            challenge_id: ChallengeId::new(1),
            cooldown_until: time(50),
        },
    ];

    let mut now = 2;
    let mut state = state;
    for event in stale_events {
        let (next, effects) = transition(state, event, time(now)).unwrap();
        assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(next.binding(), Some(current));
        assert!(!effects.iter().any(Effect::is_start_challenge));
        assert!(!effects.iter().any(Effect::is_create_permit));
        state = next;
        now += 1;
    }
}

#[test]
fn cooldown_is_closed_before_the_boundary_and_open_at_or_after_it() {
    let current = binding(7, 41, 501);
    let state = challenging(current, 22);
    let (cooling_down, effects) = transition(
        state,
        Event::ChallengeFailed {
            binding: current,
            challenge_id: ChallengeId::new(22),
            cooldown_until: time(10),
        },
        time(4),
    )
    .unwrap();
    assert!(effects.is_empty());
    assert_eq!(cooling_down.cooldown_until(), Some(time(10)));

    let (before, effects) = transition(
        cooling_down.clone(),
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(23),
            deadline: time(50),
        },
        time(9),
    )
    .unwrap();
    assert_eq!(before.phase(), UnlockPhase::LockedArmed);
    assert!(effects.is_empty());

    let (equal, effects) = transition(
        cooling_down.clone(),
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(23),
            deadline: time(50),
        },
        time(10),
    )
    .unwrap();
    assert_eq!(equal.phase(), UnlockPhase::Challenging);
    assert_eq!(effects.len(), 1);
    assert!(effects[0].is_start_challenge());

    let (after, effects) = transition(
        cooling_down,
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(24),
            deadline: time(50),
        },
        time(11),
    )
    .unwrap();
    assert_eq!(after.phase(), UnlockPhase::Challenging);
    assert_eq!(effects.len(), 1);
    assert!(effects[0].is_start_challenge());
}

#[test]
fn duplicate_near_while_challenging_does_not_start_another_challenge() {
    let current = binding(7, 41, 501);
    let state = challenging(current, 22);

    let (next, effects) = transition(
        state,
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(23),
            deadline: time(60),
        },
        time(4),
    )
    .unwrap();

    assert_eq!(next.phase(), UnlockPhase::Challenging);
    assert_eq!(next.challenge_id(), Some(ChallengeId::new(22)));
    assert!(effects.is_empty());
}

#[test]
fn session_unlock_clears_all_locked_state() {
    let current = binding(7, 41, 501);

    let (next, effects) =
        transition(challenging(current, 22), Event::SessionUnlocked, time(4)).unwrap();

    assert_eq!(next.phase(), UnlockPhase::Unlocked);
    assert_eq!(next.binding(), None);
    assert_eq!(effects, vec![Effect::ClearPermit]);
}

#[test]
fn logout_clears_all_locked_state() {
    let current = binding(7, 41, 501);

    let (next, effects) = transition(challenging(current, 22), Event::Logout, time(4)).unwrap();

    assert_eq!(next.phase(), UnlockPhase::Unlocked);
    assert_eq!(next.binding(), None);
    assert_eq!(effects, vec![Effect::ClearPermit]);
}

#[test]
fn fast_user_switch_clears_old_state_and_uses_the_new_binding() {
    let old = binding(7, 41, 501);
    let new = binding(8, 42, 502);

    let (next, effects) = transition(
        challenging(old, 22),
        Event::FastUserSwitch {
            locked_binding: Some(new),
        },
        time(4),
    )
    .unwrap();

    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(next.binding(), Some(new));
    assert_eq!(effects, vec![Effect::ClearPermit]);
}

#[test]
fn restart_when_session_is_not_locked_returns_to_unlocked() {
    let current = binding(7, 41, 501);

    let (next, effects) = transition(
        challenging(current, 22),
        Event::ServiceRestarted {
            locked_binding: None,
        },
        time(4),
    )
    .unwrap();

    assert_eq!(next.phase(), UnlockPhase::Unlocked);
    assert_eq!(effects, vec![Effect::ClearPermit]);
}

#[test]
fn non_monotonic_now_is_rejected_without_emitting_effects() {
    let current = binding(7, 41, 501);
    let (state, _) = transition(
        UnlockState::unlocked(),
        Event::SessionLocked { binding: current },
        time(10),
    )
    .unwrap();

    let error = transition(state, Event::FarStable { binding: current }, time(9)).unwrap_err();

    assert_eq!(
        error,
        TransitionError::NonMonotonicTime {
            previous: time(10),
            current: time(9),
        }
    );
}

#[test]
fn challenge_deadline_must_be_strictly_in_the_future() {
    let current = binding(7, 41, 501);

    let error = transition(
        locked_armed(current),
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(22),
            deadline: time(3),
        },
        time(3),
    )
    .unwrap_err();

    assert_eq!(
        error,
        TransitionError::DeadlineNotInFuture {
            now: time(3),
            deadline: time(3),
        }
    );
}
