use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};
use repose_unlock_core::state_machine::{
    Effect, Event, SessionBinding, TimingField, TimingOperation, TimingPolicy, TimingPolicyError,
    TransitionErrorKind, UnlockPhase, UnlockState, transition,
};

fn time(value: u64) -> MonoMillis {
    MonoMillis::new(value)
}

fn policy() -> TimingPolicy {
    TimingPolicy::new(20, 3, 6).unwrap()
}

fn abort_and_clear() -> Vec<Effect> {
    vec![Effect::AbortAllChallenges, Effect::ClearPermit]
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
        UnlockState::unlocked(policy()),
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

fn challenging(binding: SessionBinding) -> UnlockState {
    transition(
        locked_armed(binding),
        Event::NearStable { binding },
        time(3),
    )
    .unwrap()
    .0
}

fn cooling_down(binding: SessionBinding) -> UnlockState {
    let state = challenging(binding);
    let challenge_id = state.challenge_id().unwrap();
    transition(
        state,
        Event::ChallengeFailed {
            binding,
            challenge_id,
        },
        time(4),
    )
    .unwrap()
    .0
}

fn naturally_timed_out_and_retired(binding: SessionBinding) -> UnlockState {
    let state = challenging(binding);
    let challenge_id = state.challenge_id().unwrap();
    let cancelling = transition(state, Event::Tick, time(23)).unwrap().0;
    transition(
        cancelling,
        Event::ChallengeTerminated {
            binding,
            challenge_id,
        },
        time(25),
    )
    .unwrap()
    .0
}

#[test]
fn near_without_observed_departure_does_not_start_a_challenge() {
    let current = binding(7, 41, 501);
    let (next, effects) = transition(
        locked_unarmed(current),
        Event::NearStable { binding: current },
        time(2),
    )
    .unwrap();
    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert!(!effects.iter().any(Effect::is_start_challenge));
}

#[test]
fn departure_then_return_starts_exactly_one_reducer_owned_challenge() {
    let current = binding(7, 41, 501);
    let (next, effects) = transition(
        locked_armed(current),
        Event::NearStable { binding: current },
        time(3),
    )
    .unwrap();
    let [Effect::StartChallenge(request)] = effects.as_slice() else {
        panic!("expected exactly one start-challenge effect");
    };
    assert_eq!(next.phase(), UnlockPhase::Challenging);
    assert_eq!(request.binding(), current);
    assert_eq!(request.challenge_id().get(), 1);
    assert_eq!(request.deadline(), time(23));
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
    let (next, effects) = transition(
        challenging(current),
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
    assert_eq!(effects, abort_and_clear());
}

#[test]
fn uid_change_resets_a_live_challenge_to_unarmed() {
    let old = binding(7, 41, 501);
    let changed_uid = binding(8, 41, 502);
    let (next, effects) = transition(
        challenging(old),
        Event::SessionLocked {
            binding: changed_uid,
        },
        time(4),
    )
    .unwrap();
    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(next.binding(), Some(changed_uid));
    assert_eq!(effects, abort_and_clear());
}

#[test]
fn audit_session_change_resets_a_live_challenge_to_unarmed() {
    let old = binding(7, 41, 501);
    let changed_audit_session = binding(8, 42, 501);
    let (next, effects) = transition(
        challenging(old),
        Event::SessionLocked {
            binding: changed_audit_session,
        },
        time(4),
    )
    .unwrap();
    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(next.binding(), Some(changed_audit_session));
    assert_eq!(effects, abort_and_clear());
}

#[test]
fn stale_epoch_proximity_and_worker_events_are_inert() {
    let current = binding(8, 41, 501);
    let stale = binding(7, 41, 501);
    let stale_challenge = challenging(stale);
    let stale_id = stale_challenge.challenge_id().unwrap();
    let (mut state, _) = transition(
        stale_challenge,
        Event::SessionLocked { binding: current },
        time(4),
    )
    .unwrap();
    let stale_events = [
        Event::FarStable { binding: stale },
        Event::ReliableDisconnect { binding: stale },
        Event::NearStable { binding: stale },
        Event::ChallengeFailed {
            binding: stale,
            challenge_id: stale_id,
        },
        Event::ChallengeTimedOut {
            binding: stale,
            challenge_id: stale_id,
        },
        Event::ChallengeTerminated {
            binding: stale,
            challenge_id: stale_id,
        },
    ];

    for (offset, event) in stale_events.into_iter().enumerate() {
        let (next, effects) = transition(state, event, time(5 + offset as u64)).unwrap();
        assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(next.binding(), Some(current));
        assert!(!effects.iter().any(Effect::is_start_challenge));
        assert!(!effects.iter().any(Effect::is_create_permit));
        state = next;
    }
}

#[test]
fn cooldown_is_closed_before_the_boundary_and_open_at_or_after_it() {
    let current = binding(7, 41, 501);
    let cooling_state = cooling_down(current);
    assert_eq!(cooling_state.cooldown_until(), Some(time(10)));

    let (before, effects) = transition(
        cooling_state,
        Event::NearStable { binding: current },
        time(9),
    )
    .unwrap();
    assert_eq!(before.phase(), UnlockPhase::LockedArmed);
    assert!(effects.is_empty());

    for now in [10, 11] {
        let (next, effects) = transition(
            cooling_down(current),
            Event::NearStable { binding: current },
            time(now),
        )
        .unwrap();
        assert_eq!(next.phase(), UnlockPhase::Challenging);
        assert_eq!(effects.len(), 1);
        assert!(effects[0].is_start_challenge());
    }
}

#[test]
fn duplicate_near_while_challenging_does_not_start_another_challenge() {
    let current = binding(7, 41, 501);
    let state = challenging(current);
    let challenge_id = state.challenge_id();
    let (next, effects) =
        transition(state, Event::NearStable { binding: current }, time(4)).unwrap();
    assert_eq!(next.phase(), UnlockPhase::Challenging);
    assert_eq!(next.challenge_id(), challenge_id);
    assert!(effects.is_empty());
}

#[test]
fn session_unlock_and_logout_clear_all_locked_state() {
    let current = binding(7, 41, 501);
    for event in [Event::SessionUnlocked, Event::Logout] {
        let (next, effects) = transition(challenging(current), event, time(4)).unwrap();
        assert_eq!(next.phase(), UnlockPhase::Unlocked);
        assert_eq!(next.binding(), None);
        assert_eq!(effects, abort_and_clear());
    }
}

#[test]
fn fast_user_switch_clears_old_state_and_uses_the_new_binding() {
    let old = binding(7, 41, 501);
    let new = binding(8, 42, 502);
    let (next, effects) = transition(
        challenging(old),
        Event::FastUserSwitch {
            locked_binding: Some(new),
        },
        time(4),
    )
    .unwrap();
    assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(next.binding(), Some(new));
    assert_eq!(effects, abort_and_clear());
}

#[test]
fn restart_when_session_is_not_locked_returns_to_unlocked() {
    let current = binding(7, 41, 501);
    let (next, effects) = transition(
        challenging(current),
        Event::ServiceRestarted {
            locked_binding: None,
        },
        time(4),
    )
    .unwrap();
    assert_eq!(next.phase(), UnlockPhase::Unlocked);
    assert_eq!(effects, abort_and_clear());
}

#[test]
fn transition_error_returns_the_fenced_authoritative_state_to_the_caller() {
    let current = binding(8, 41, 501);
    let replacement = binding(9, 42, 502);
    let active = challenging(current);
    let first_id = active.challenge_id().unwrap();
    let (fenced, effects) = transition(
        active,
        Event::ServiceRestarted {
            locked_binding: Some(replacement),
        },
        time(10),
    )
    .unwrap();
    assert_eq!(fenced.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(effects, abort_and_clear());

    let error = transition(
        fenced,
        Event::FarStable {
            binding: replacement,
        },
        time(9),
    )
    .unwrap_err();
    assert_eq!(
        error.kind(),
        TransitionErrorKind::NonMonotonicTime {
            previous: time(10),
            current: time(9),
        }
    );
    let (recovered, kind) = error.into_parts();
    assert_eq!(
        kind,
        TransitionErrorKind::NonMonotonicTime {
            previous: time(10),
            current: time(9),
        }
    );
    assert_eq!(recovered.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(recovered.binding(), Some(replacement));
    assert_eq!(recovered.authoritative_binding(), Some(replacement));
    assert_eq!(
        recovered.highest_lock_epoch(),
        Some(replacement.lock_epoch())
    );
    assert_eq!(recovered.last_observed_at(), Some(time(10)));

    let (still_fenced, effects) = transition(
        recovered,
        Event::FarStable {
            binding: replacement,
        },
        time(11),
    )
    .unwrap();
    assert!(effects.is_empty());
    let (still_fenced, effects) = transition(
        still_fenced,
        Event::NearStable {
            binding: replacement,
        },
        time(12),
    )
    .unwrap();
    assert!(!effects.iter().any(Effect::is_start_challenge));

    let (retired, effects) = transition(
        still_fenced,
        Event::ChallengeTerminated {
            binding: current,
            challenge_id: first_id,
        },
        time(13),
    )
    .unwrap();
    assert!(effects.is_empty());
    let (armed, _) = transition(
        retired,
        Event::FarStable {
            binding: replacement,
        },
        time(14),
    )
    .unwrap();
    let (next, effects) = transition(
        armed,
        Event::NearStable {
            binding: replacement,
        },
        time(15),
    )
    .unwrap();
    assert_eq!(next.phase(), UnlockPhase::Challenging);
    assert_eq!(next.challenge_id().unwrap().get(), first_id.get() + 1);
    assert_eq!(effects.len(), 1);
    assert!(effects[0].is_start_challenge());
}

#[test]
fn stale_lifecycle_epoch_cannot_replace_the_current_locked_binding() {
    let current = binding(8, 42, 502);
    let stale = binding(7, 41, 501);
    let lifecycle_events = [
        Event::SessionLocked { binding: stale },
        Event::FastUserSwitch {
            locked_binding: Some(stale),
        },
        Event::ServiceRestarted {
            locked_binding: Some(stale),
        },
    ];

    for event in lifecycle_events {
        let (next, effects) = transition(challenging(current), event, time(4)).unwrap();
        assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(next.binding(), Some(current));
        assert_eq!(next.challenge_id(), None);
        assert_eq!(effects, abort_and_clear());

        let (after_far, effects) =
            transition(next, Event::FarStable { binding: stale }, time(5)).unwrap();
        assert_eq!(after_far.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(after_far.binding(), Some(current));
        assert!(effects.is_empty());
    }
}

#[test]
fn epoch_high_water_survives_unlock_logout_and_unlocked_switches() {
    let current = binding(8, 42, 502);
    let stale = binding(7, 41, 501);
    let clearing_events = [
        Event::SessionUnlocked,
        Event::Logout,
        Event::FastUserSwitch {
            locked_binding: None,
        },
        Event::ServiceRestarted {
            locked_binding: None,
        },
    ];

    for clearing_event in clearing_events {
        let (unlocked, _) = transition(locked_unarmed(current), clearing_event, time(2)).unwrap();
        let (next, effects) =
            transition(unlocked, Event::SessionLocked { binding: stale }, time(3)).unwrap();
        assert_eq!(next.phase(), UnlockPhase::Unlocked);
        assert_eq!(next.binding(), None);
        assert_eq!(next.highest_lock_epoch(), Some(LockEpoch::new(8)));
        assert_eq!(effects, vec![Effect::ClearPermit]);
    }
}

#[test]
fn equal_epoch_accepts_only_the_identical_binding_and_higher_epoch_replaces_it() {
    let current = binding(8, 41, 501);
    let equal_epoch_new_uid = binding(8, 41, 502);
    let equal_epoch_new_audit_session = binding(8, 42, 501);
    let higher_binding = binding(9, 43, 503);

    let (identical, effects) = transition(
        challenging(current),
        Event::SessionLocked { binding: current },
        time(4),
    )
    .unwrap();
    assert_eq!(identical.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(identical.binding(), Some(current));
    assert_eq!(effects, abort_and_clear());

    for incompatible in [equal_epoch_new_uid, equal_epoch_new_audit_session] {
        let (rejected, effects) = transition(
            challenging(current),
            Event::SessionLocked {
                binding: incompatible,
            },
            time(4),
        )
        .unwrap();
        assert_eq!(rejected.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(rejected.binding(), Some(current));
        assert_eq!(effects, abort_and_clear());
    }

    let (higher, effects) = transition(
        identical,
        Event::SessionLocked {
            binding: higher_binding,
        },
        time(5),
    )
    .unwrap();
    assert_eq!(higher.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(higher.binding(), Some(higher_binding));
    assert_eq!(effects, abort_and_clear());
}

#[test]
fn challenge_must_terminate_before_a_new_attempt_can_start() {
    let current = binding(7, 41, 501);
    let (first_state, first_effects) = transition(
        locked_armed(current),
        Event::NearStable { binding: current },
        time(3),
    )
    .unwrap();
    let [Effect::StartChallenge(first)] = first_effects.as_slice() else {
        panic!("expected exactly one first challenge");
    };
    let first_id = first.challenge_id();

    let (cancelling, effects) =
        transition(first_state, Event::FarStable { binding: current }, time(4)).unwrap();
    assert_eq!(cancelling.phase(), UnlockPhase::Cancelling);
    let [Effect::CancelChallenge(cancel)] = effects.as_slice() else {
        panic!("expected exactly one cancel-challenge effect");
    };
    assert_eq!(cancel.binding(), first.binding());
    assert_eq!(cancel.challenge_id(), first.challenge_id());
    assert_eq!(cancel.deadline(), first.deadline());

    let (still_cancelling, effects) =
        transition(cancelling, Event::NearStable { binding: current }, time(5)).unwrap();
    assert_eq!(still_cancelling.phase(), UnlockPhase::Cancelling);
    assert!(effects.is_empty());

    let (armed_again, effects) = transition(
        still_cancelling,
        Event::ChallengeTerminated {
            binding: current,
            challenge_id: first_id,
        },
        time(6),
    )
    .unwrap();
    assert_eq!(armed_again.phase(), UnlockPhase::LockedArmed);
    assert!(effects.is_empty());

    let (challenging_again, effects) =
        transition(armed_again, Event::NearStable { binding: current }, time(7)).unwrap();
    let [Effect::StartChallenge(second)] = effects.as_slice() else {
        panic!("expected exactly one second challenge");
    };
    assert_eq!(challenging_again.phase(), UnlockPhase::Challenging);
    assert_ne!(second.challenge_id(), first_id);
}

#[test]
fn timing_policy_validates_zero_and_maximum_values() {
    assert_eq!(
        TimingPolicy::new(0, 3_000, 2_000),
        Err(TimingPolicyError::Zero {
            field: TimingField::ChallengeTtl,
        })
    );
    assert!(
        TimingPolicy::new(
            TimingPolicy::MAX_CHALLENGE_TTL_MS,
            TimingPolicy::MAX_PERMIT_TTL_MS,
            TimingPolicy::MAX_COOLDOWN_MS,
        )
        .is_ok()
    );
    assert_eq!(
        TimingPolicy::new(TimingPolicy::MAX_CHALLENGE_TTL_MS + 1, 3_000, 2_000),
        Err(TimingPolicyError::ExceedsMaximum {
            field: TimingField::ChallengeTtl,
            value: TimingPolicy::MAX_CHALLENGE_TTL_MS + 1,
            maximum: TimingPolicy::MAX_CHALLENGE_TTL_MS,
        })
    );
    assert_eq!(
        TimingPolicy::new(5_000, 0, 2_000),
        Err(TimingPolicyError::Zero {
            field: TimingField::PermitTtl,
        })
    );
    assert_eq!(
        TimingPolicy::new(5_000, TimingPolicy::MAX_PERMIT_TTL_MS + 1, 2_000),
        Err(TimingPolicyError::ExceedsMaximum {
            field: TimingField::PermitTtl,
            value: TimingPolicy::MAX_PERMIT_TTL_MS + 1,
            maximum: TimingPolicy::MAX_PERMIT_TTL_MS,
        })
    );
    assert_eq!(
        TimingPolicy::new(5_000, 3_000, 0),
        Err(TimingPolicyError::Zero {
            field: TimingField::Cooldown,
        })
    );
    assert_eq!(
        TimingPolicy::new(5_000, 3_000, TimingPolicy::MAX_COOLDOWN_MS + 1),
        Err(TimingPolicyError::ExceedsMaximum {
            field: TimingField::Cooldown,
            value: TimingPolicy::MAX_COOLDOWN_MS + 1,
            maximum: TimingPolicy::MAX_COOLDOWN_MS,
        })
    );
}

#[test]
fn challenge_naturally_cancels_at_or_after_its_deadline() {
    let current = binding(7, 41, 501);
    let state = challenging(current);
    let request_id = state.challenge_id().unwrap();

    let (before, effects) = transition(state, Event::Tick, time(22)).unwrap();
    assert_eq!(before.phase(), UnlockPhase::Challenging);
    assert!(effects.is_empty());

    for now in [23, 24] {
        let (next, effects) = transition(challenging(current), Event::Tick, time(now)).unwrap();
        assert_eq!(next.phase(), UnlockPhase::Cancelling);
        assert_eq!(next.cooldown_until(), Some(time(now + 6)));
        assert_eq!(effects.len(), 1);
        assert!(effects[0].is_cancel_challenge());
        assert_eq!(next.challenge_id(), Some(request_id));
    }
}

#[test]
fn natural_timeout_cannot_wedge_or_start_again_before_termination_and_cooldown() {
    let current = binding(7, 41, 501);
    let state = challenging(current);
    let first_id = state.challenge_id().unwrap();
    let (cancelling, _) = transition(state, Event::Tick, time(23)).unwrap();

    let (still_cancelling, effects) =
        transition(cancelling, Event::NearStable { binding: current }, time(24)).unwrap();
    assert_eq!(still_cancelling.phase(), UnlockPhase::Cancelling);
    assert!(effects.is_empty());

    let (cooling_down, _) = transition(
        still_cancelling,
        Event::ChallengeTerminated {
            binding: current,
            challenge_id: first_id,
        },
        time(25),
    )
    .unwrap();
    assert_eq!(cooling_down.phase(), UnlockPhase::LockedArmed);
    assert_eq!(cooling_down.cooldown_until(), Some(time(29)));

    let (before, effects) = transition(
        cooling_down,
        Event::NearStable { binding: current },
        time(28),
    )
    .unwrap();
    assert_eq!(before.phase(), UnlockPhase::LockedArmed);
    assert!(effects.is_empty());

    let (at_boundary, effects) = transition(
        naturally_timed_out_and_retired(current),
        Event::NearStable { binding: current },
        time(29),
    )
    .unwrap();
    assert_eq!(at_boundary.phase(), UnlockPhase::Challenging);
    assert_eq!(effects.len(), 1);
    assert!(effects[0].is_start_challenge());
    assert_ne!(at_boundary.challenge_id(), Some(first_id));
}

#[test]
fn challenge_deadline_overflow_returns_armed_state_without_advancing_the_counter() {
    let current = binding(7, 41, 501);
    let near_overflow_now = u64::MAX - 10;
    let (locked, _) = transition(
        UnlockState::unlocked(policy()),
        Event::SessionLocked { binding: current },
        time(1),
    )
    .unwrap();
    let (armed, _) = transition(locked, Event::FarStable { binding: current }, time(2)).unwrap();
    let error = transition(
        armed,
        Event::NearStable { binding: current },
        time(near_overflow_now),
    )
    .unwrap_err();
    assert_eq!(
        error.kind(),
        TransitionErrorKind::TimeOverflow {
            operation: TimingOperation::ChallengeDeadline,
            now: time(near_overflow_now),
            duration_ms: policy().challenge_ttl_ms(),
        }
    );
    let recovered = error.into_state();
    assert_eq!(recovered.phase(), UnlockPhase::LockedArmed);
    assert_eq!(recovered.binding(), Some(current));
    assert_eq!(recovered.authoritative_binding(), Some(current));
    assert_eq!(recovered.last_observed_at(), Some(time(2)));

    let (challenging, effects) =
        transition(recovered, Event::NearStable { binding: current }, time(3)).unwrap();
    assert_eq!(challenging.challenge_id().unwrap().get(), 1);
    assert_eq!(effects.len(), 1);
    assert!(effects[0].is_start_challenge());
}

#[test]
fn cooldown_overflow_returns_the_original_challenge_and_blocks_a_second_worker() {
    let current = binding(7, 41, 501);
    let cooldown_now = u64::MAX - 2;

    for termination_kind in 0..3 {
        let state = challenging(current);
        let challenge_id = state.challenge_id().unwrap();
        let event = match termination_kind {
            0 => Event::ChallengeFailed {
                binding: current,
                challenge_id,
            },
            1 => Event::ChallengeTimedOut {
                binding: current,
                challenge_id,
            },
            _ => Event::ChallengeTerminated {
                binding: current,
                challenge_id,
            },
        };
        let error = transition(state, event, time(cooldown_now)).unwrap_err();
        assert_eq!(
            error.kind(),
            TransitionErrorKind::TimeOverflow {
                operation: TimingOperation::CooldownDeadline,
                now: time(cooldown_now),
                duration_ms: policy().cooldown_ms(),
            }
        );
        let recovered = error.into_state();
        assert_eq!(recovered.phase(), UnlockPhase::Challenging);
        assert_eq!(recovered.challenge_id(), Some(challenge_id));
        assert_eq!(recovered.authoritative_binding(), Some(current));
        assert_eq!(recovered.last_observed_at(), Some(time(3)));

        let (still_challenging, effects) =
            transition(recovered, Event::NearStable { binding: current }, time(4)).unwrap();
        assert_eq!(still_challenging.phase(), UnlockPhase::Challenging);
        assert_eq!(still_challenging.challenge_id(), Some(challenge_id));
        assert!(!effects.iter().any(Effect::is_start_challenge));

        let (retired, effects) = transition(
            still_challenging,
            Event::ChallengeTerminated {
                binding: current,
                challenge_id,
            },
            time(5),
        )
        .unwrap();
        assert_eq!(retired.phase(), UnlockPhase::LockedArmed);
        assert_eq!(retired.binding(), Some(current));
        assert_eq!(retired.cooldown_until(), Some(time(11)));
        assert!(effects.is_empty());
    }

    let state = challenging(current);
    let challenge_id = state.challenge_id().unwrap();
    let error = transition(state, Event::Tick, time(cooldown_now)).unwrap_err();
    assert_eq!(
        error.kind(),
        TransitionErrorKind::TimeOverflow {
            operation: TimingOperation::CooldownDeadline,
            now: time(cooldown_now),
            duration_ms: policy().cooldown_ms(),
        }
    );
    let recovered = error.into_state();
    assert_eq!(recovered.phase(), UnlockPhase::Challenging);
    assert_eq!(recovered.challenge_id(), Some(challenge_id));
    assert_eq!(recovered.last_observed_at(), Some(time(3)));
    let (still_challenging, effects) = transition(recovered, Event::Tick, time(4)).unwrap();
    assert_eq!(still_challenging.phase(), UnlockPhase::Challenging);
    assert!(effects.is_empty());
}

#[test]
fn authoritative_resets_remain_infallible_after_a_transition_error() {
    let current = binding(7, 41, 501);
    let state = challenging(current);
    let challenge_id = state.challenge_id().unwrap();
    let error = transition(
        state,
        Event::ChallengeFailed {
            binding: current,
            challenge_id,
        },
        time(u64::MAX - 2),
    )
    .unwrap_err();
    assert!(matches!(
        error.kind(),
        TransitionErrorKind::TimeOverflow {
            operation: TimingOperation::CooldownDeadline,
            ..
        }
    ));

    let (reset, effects) = transition(error.into_state(), Event::SessionUnlocked, time(1)).unwrap();
    assert_eq!(reset.phase(), UnlockPhase::Unlocked);
    assert_eq!(reset.last_observed_at(), Some(time(1)));
    assert_eq!(effects, abort_and_clear());
}

#[test]
fn global_resets_abort_and_fence_challenging_or_cancelling_workers() {
    let current = binding(7, 41, 501);
    let replacement = binding(8, 42, 502);
    let resets = [
        (
            Event::SessionLocked { binding: current },
            UnlockPhase::LockedUnarmed,
        ),
        (Event::SessionUnlocked, UnlockPhase::Unlocked),
        (Event::Logout, UnlockPhase::Unlocked),
        (
            Event::FastUserSwitch {
                locked_binding: Some(replacement),
            },
            UnlockPhase::LockedUnarmed,
        ),
        (
            Event::ServiceRestarted {
                locked_binding: Some(current),
            },
            UnlockPhase::LockedUnarmed,
        ),
    ];

    for start_cancelling in [false, true] {
        for (reset, expected_phase) in resets {
            let challenge = challenging(current);
            let state = if start_cancelling {
                transition(challenge, Event::FarStable { binding: current }, time(4))
                    .unwrap()
                    .0
            } else {
                challenge
            };
            let reset_at = if start_cancelling { 5 } else { 4 };
            let (next, effects) = transition(state, reset, time(reset_at)).unwrap();
            assert_eq!(next.phase(), expected_phase);
            assert_eq!(next.challenge_id(), None);
            assert_eq!(
                effects,
                vec![Effect::AbortAllChallenges, Effect::ClearPermit]
            );
        }
    }
}

#[test]
fn reset_fence_blocks_new_work_until_the_exact_old_worker_terminates() {
    let current = binding(7, 41, 501);
    let mismatch = binding(8, 42, 502);
    let active = challenging(current);
    let first_id = active.challenge_id().unwrap();
    let (unlocked, effects) = transition(active, Event::SessionUnlocked, time(4)).unwrap();
    assert_eq!(unlocked.phase(), UnlockPhase::Unlocked);
    assert_eq!(
        effects,
        vec![Effect::AbortAllChallenges, Effect::ClearPermit]
    );

    let (locked, effects) =
        transition(unlocked, Event::SessionLocked { binding: current }, time(5)).unwrap();
    assert_eq!(locked.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(
        effects,
        vec![Effect::AbortAllChallenges, Effect::ClearPermit]
    );

    let (after_far, effects) =
        transition(locked, Event::FarStable { binding: current }, time(6)).unwrap();
    assert_eq!(after_far.phase(), UnlockPhase::LockedUnarmed);
    assert!(effects.is_empty());
    let (after_near, effects) =
        transition(after_far, Event::NearStable { binding: current }, time(7)).unwrap();
    assert_eq!(after_near.phase(), UnlockPhase::LockedUnarmed);
    assert!(!effects.iter().any(Effect::is_start_challenge));

    let (still_fenced, effects) = transition(
        after_near,
        Event::ChallengeTerminated {
            binding: mismatch,
            challenge_id: first_id,
        },
        time(8),
    )
    .unwrap();
    assert_eq!(still_fenced.phase(), UnlockPhase::LockedUnarmed);
    assert!(effects.is_empty());

    let (retired, effects) = transition(
        still_fenced,
        Event::ChallengeTerminated {
            binding: current,
            challenge_id: first_id,
        },
        time(9),
    )
    .unwrap();
    assert_eq!(retired.phase(), UnlockPhase::LockedUnarmed);
    assert!(effects.is_empty());

    let (armed, _) = transition(retired, Event::FarStable { binding: current }, time(10)).unwrap();
    let (next, effects) =
        transition(armed, Event::NearStable { binding: current }, time(11)).unwrap();
    assert_eq!(next.phase(), UnlockPhase::Challenging);
    assert_ne!(next.challenge_id(), Some(first_id));
    assert_eq!(effects.len(), 1);
    assert!(effects[0].is_start_challenge());
}
