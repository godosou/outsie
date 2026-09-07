use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};
use repose_unlock_core::state_machine::{
    Effect, Event, SessionBinding, TimingPolicy, UnlockPhase, UnlockState, transition,
};

fn time(value: u64) -> MonoMillis {
    MonoMillis::new(value)
}

fn policy() -> TimingPolicy {
    TimingPolicy::new(100, 3, 20).unwrap()
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

fn generated_public_event(selector: u64, state: &UnlockState) -> Event {
    let bindings = [
        binding(7, 41, 501),
        binding(6, 41, 501),
        binding(8, 41, 502),
        binding(8, 42, 501),
    ];
    let selected = bindings[(selector as usize) % bindings.len()];

    match (selector / 4) % 16 {
        0 => Event::SessionLocked { binding: selected },
        1 => Event::FarStable { binding: selected },
        2 => Event::ReliableDisconnect { binding: selected },
        3 => Event::NearStable { binding: selected },
        4 => state
            .challenge_id()
            .map_or(Event::SessionUnlocked, |id| Event::ChallengeFailed {
                binding: selected,
                challenge_id: id,
            }),
        5 => state
            .challenge_id()
            .map_or(Event::Logout, |id| Event::ChallengeTimedOut {
                binding: selected,
                challenge_id: id,
            }),
        6 => state
            .challenge_id()
            .map_or(Event::SessionUnlocked, |id| Event::ChallengeTerminated {
                binding: selected,
                challenge_id: id,
            }),
        7 => state
            .challenge_id()
            .map_or(Event::Logout, |id| Event::PermitConsumed {
                binding: selected,
                challenge_id: id,
            }),
        8 => state
            .challenge_id()
            .map_or(Event::SessionUnlocked, |id| Event::PermitExpired {
                binding: selected,
                challenge_id: id,
            }),
        9 => Event::SessionUnlocked,
        10 => Event::Logout,
        11 => Event::FastUserSwitch {
            locked_binding: Some(selected),
        },
        12 => Event::FastUserSwitch {
            locked_binding: None,
        },
        13 => Event::ServiceRestarted {
            locked_binding: Some(selected),
        },
        14 => Event::ServiceRestarted {
            locked_binding: None,
        },
        _ => Event::Tick,
    }
}

fn challenging(current: SessionBinding) -> UnlockState {
    let (locked, _) = transition(
        UnlockState::unlocked(policy()),
        Event::SessionLocked { binding: current },
        time(1),
    )
    .unwrap();
    let (armed, _) = transition(locked, Event::FarStable { binding: current }, time(2)).unwrap();
    transition(armed, Event::NearStable { binding: current }, time(3))
        .unwrap()
        .0
}

fn locked_unarmed(current: SessionBinding) -> UnlockState {
    transition(
        UnlockState::unlocked(policy()),
        Event::SessionLocked { binding: current },
        time(1),
    )
    .unwrap()
    .0
}

fn locked_armed(current: SessionBinding) -> UnlockState {
    transition(
        locked_unarmed(current),
        Event::FarStable { binding: current },
        time(2),
    )
    .unwrap()
    .0
}

fn cancelling(current: SessionBinding) -> UnlockState {
    transition(
        challenging(current),
        Event::FarStable { binding: current },
        time(4),
    )
    .unwrap()
    .0
}

#[test]
fn arbitrary_public_event_sequences_never_create_a_permit_without_verified_proof() {
    for seed in 0_u64..1_024 {
        let mut state = UnlockState::unlocked(policy());
        let mut generator = seed | 1;

        for step in 1_u64..=64 {
            generator ^= generator << 13;
            generator ^= generator >> 7;
            generator ^= generator << 17;
            let event = generated_public_event(generator, &state);
            let (next, effects) = transition(state, event, time(step)).unwrap();
            assert!(
                !effects.iter().any(Effect::is_create_permit),
                "seed {seed}, step {step} emitted CreatePermit"
            );
            state = next;
        }
    }
}

#[test]
fn changing_any_binding_component_makes_old_challenge_events_inert() {
    let old = binding(7, 41, 501);
    let replacements = [
        binding(8, 41, 501),
        binding(8, 41, 502),
        binding(8, 42, 501),
    ];

    for replacement in replacements {
        let old_state = challenging(old);
        let old_id = old_state.challenge_id().unwrap();
        let (mut state, _) = transition(
            old_state,
            Event::SessionLocked {
                binding: replacement,
            },
            time(4),
        )
        .unwrap();
        let prior_events = [
            Event::FarStable { binding: old },
            Event::NearStable { binding: old },
            Event::ChallengeFailed {
                binding: old,
                challenge_id: old_id,
            },
            Event::ChallengeTimedOut {
                binding: old,
                challenge_id: old_id,
            },
            Event::ChallengeTerminated {
                binding: old,
                challenge_id: old_id,
            },
            Event::PermitConsumed {
                binding: old,
                challenge_id: old_id,
            },
            Event::PermitExpired {
                binding: old,
                challenge_id: old_id,
            },
        ];

        for (offset, event) in prior_events.into_iter().enumerate() {
            let (next, effects) = transition(state, event, time(5 + offset as u64)).unwrap();
            assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
            assert_eq!(next.binding(), Some(replacement));
            assert!(!effects.iter().any(Effect::is_create_permit));
            state = next;
        }
    }
}

#[test]
fn duplicate_near_events_never_create_concurrent_challenges() {
    let current = binding(7, 41, 501);
    let mut state = challenging(current);
    let first_id = state.challenge_id().unwrap();

    for step in 4_u64..64 {
        let was_challenging = state.phase() == UnlockPhase::Challenging;
        let (next, effects) =
            transition(state, Event::NearStable { binding: current }, time(step)).unwrap();
        if was_challenging {
            assert_eq!(
                effects
                    .iter()
                    .filter(|effect| effect.is_start_challenge())
                    .count(),
                0
            );
            assert_eq!(next.challenge_id(), Some(first_id));
        }
        state = next;
    }
}

#[test]
fn restart_cannot_preserve_any_publicly_reachable_transient_state() {
    let current = binding(7, 41, 501);
    for (state, now, had_worker) in [
        (UnlockState::unlocked(policy()), 1_u64, false),
        (locked_unarmed(current), 2, false),
        (locked_armed(current), 3, false),
        (challenging(current), 4, true),
        (cancelling(current), 5, true),
    ] {
        let (next, effects) = transition(
            state,
            Event::ServiceRestarted {
                locked_binding: Some(current),
            },
            time(now),
        )
        .unwrap();
        assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(next.binding(), Some(current));
        assert_eq!(next.challenge_id(), None);
        assert_eq!(next.permit_expires_at(), None);
        let expected = if had_worker {
            vec![Effect::AbortAllChallenges, Effect::ClearPermit]
        } else {
            vec![Effect::ClearPermit]
        };
        assert_eq!(effects, expected);
    }
}

#[test]
fn every_authoritative_reset_rebases_backward_time_and_clears_a_challenge() {
    let current = binding(7, 41, 501);
    let replacement = binding(8, 42, 502);
    let resets = [
        (Event::SessionUnlocked, UnlockPhase::Unlocked, None),
        (Event::Logout, UnlockPhase::Unlocked, None),
        (
            Event::FastUserSwitch {
                locked_binding: None,
            },
            UnlockPhase::Unlocked,
            None,
        ),
        (
            Event::FastUserSwitch {
                locked_binding: Some(replacement),
            },
            UnlockPhase::LockedUnarmed,
            Some(replacement),
        ),
        (
            Event::ServiceRestarted {
                locked_binding: None,
            },
            UnlockPhase::Unlocked,
            None,
        ),
        (
            Event::ServiceRestarted {
                locked_binding: Some(current),
            },
            UnlockPhase::LockedUnarmed,
            Some(current),
        ),
    ];

    for (reset, expected_phase, expected_binding) in resets {
        let (next, effects) = transition(challenging(current), reset, time(2)).unwrap();
        assert_eq!(next.phase(), expected_phase);
        assert_eq!(next.binding(), expected_binding);
        assert_eq!(next.last_observed_at(), Some(time(2)));
        assert_eq!(next.challenge_id(), None);
        assert_eq!(effects, abort_and_clear());
    }
}

#[test]
fn lower_epoch_lifecycle_events_never_revive_old_bindings() {
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

    for lifecycle_event in lifecycle_events {
        let current_state = challenging(current);
        let current_id = current_state.challenge_id().unwrap();
        let (mut state, effects) = transition(current_state, lifecycle_event, time(4)).unwrap();
        assert_eq!(state.phase(), UnlockPhase::LockedUnarmed);
        assert_eq!(state.binding(), Some(current));
        assert_eq!(effects, abort_and_clear());

        let old_events = [
            Event::FarStable { binding: stale },
            Event::NearStable { binding: stale },
            Event::ChallengeFailed {
                binding: stale,
                challenge_id: current_id,
            },
            Event::ChallengeTimedOut {
                binding: stale,
                challenge_id: current_id,
            },
            Event::ChallengeTerminated {
                binding: stale,
                challenge_id: current_id,
            },
        ];

        for (offset, old_event) in old_events.into_iter().enumerate() {
            let (next, effects) = transition(state, old_event, time(5 + offset as u64)).unwrap();
            assert_eq!(next.phase(), UnlockPhase::LockedUnarmed);
            assert_eq!(next.binding(), Some(current));
            assert!(!effects.iter().any(Effect::is_start_challenge));
            assert!(!effects.iter().any(Effect::is_create_permit));
            state = next;
        }
    }
}

#[test]
fn equal_epoch_uid_or_audit_changes_never_replace_the_authoritative_binding() {
    let current = binding(8, 41, 501);
    let incompatible_bindings = [binding(8, 41, 502), binding(8, 42, 501)];

    for incompatible in incompatible_bindings {
        let lifecycle_events = [
            Event::SessionLocked {
                binding: incompatible,
            },
            Event::FastUserSwitch {
                locked_binding: Some(incompatible),
            },
            Event::ServiceRestarted {
                locked_binding: Some(incompatible),
            },
        ];

        for lifecycle_event in lifecycle_events {
            let (reset, effects) =
                transition(challenging(current), lifecycle_event, time(4)).unwrap();
            assert_eq!(reset.phase(), UnlockPhase::LockedUnarmed);
            assert_eq!(reset.binding(), Some(current));
            assert_eq!(effects, abort_and_clear());

            let (after_far, effects) = transition(
                reset,
                Event::FarStable {
                    binding: incompatible,
                },
                time(5),
            )
            .unwrap();
            assert_eq!(after_far.phase(), UnlockPhase::LockedUnarmed);
            assert_eq!(after_far.binding(), Some(current));
            assert!(!effects.iter().any(Effect::is_start_challenge));
        }

        let (unlocked, _) =
            transition(challenging(current), Event::SessionUnlocked, time(4)).unwrap();
        let (still_unlocked, effects) = transition(
            unlocked,
            Event::SessionLocked {
                binding: incompatible,
            },
            time(5),
        )
        .unwrap();
        assert_eq!(still_unlocked.phase(), UnlockPhase::Unlocked);
        assert_eq!(still_unlocked.binding(), None);
        assert_eq!(effects, abort_and_clear());
    }
}

#[test]
fn every_active_worker_reset_stays_fenced_until_matching_termination() {
    let current = binding(7, 41, 501);
    let replacement = binding(8, 42, 502);
    let reset_events = || {
        [
            Event::SessionLocked { binding: current },
            Event::SessionUnlocked,
            Event::Logout,
            Event::FastUserSwitch {
                locked_binding: Some(replacement),
            },
            Event::ServiceRestarted {
                locked_binding: Some(current),
            },
        ]
    };

    for begin_cancelling in [false, true] {
        for reset in reset_events() {
            let active = challenging(current);
            let active_id = active.challenge_id().unwrap();
            let state = if begin_cancelling {
                transition(active, Event::FarStable { binding: current }, time(4))
                    .unwrap()
                    .0
            } else {
                active
            };
            let now = if begin_cancelling { 5 } else { 4 };
            let (reset_state, effects) = transition(state, reset, time(now)).unwrap();
            assert_eq!(
                effects,
                vec![Effect::AbortAllChallenges, Effect::ClearPermit]
            );

            let outward_binding = reset_state.binding();
            let probe_binding = outward_binding.unwrap_or(current);
            let (after_far, effects) = transition(
                reset_state,
                Event::FarStable {
                    binding: probe_binding,
                },
                time(now + 1),
            )
            .unwrap();
            assert!(!effects.iter().any(Effect::is_start_challenge));
            let (after_near, effects) = transition(
                after_far,
                Event::NearStable {
                    binding: probe_binding,
                },
                time(now + 2),
            )
            .unwrap();
            assert!(!effects.iter().any(Effect::is_start_challenge));

            let (retired, effects) = transition(
                after_near,
                Event::ChallengeTerminated {
                    binding: current,
                    challenge_id: active_id,
                },
                time(now + 3),
            )
            .unwrap();
            assert!(effects.is_empty());
            assert_eq!(retired.challenge_id(), None);
        }
    }
}
