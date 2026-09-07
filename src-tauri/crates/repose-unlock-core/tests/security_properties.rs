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
    let unlocked = UnlockState::unlocked(policy());
    let (unarmed, _) = transition(
        unlocked.clone(),
        Event::SessionLocked { binding: current },
        time(1),
    )
    .unwrap();
    let (armed, _) = transition(
        unarmed.clone(),
        Event::FarStable { binding: current },
        time(2),
    )
    .unwrap();
    let challenging = challenging(current);
    let (cancelling, _) = transition(
        challenging.clone(),
        Event::FarStable { binding: current },
        time(4),
    )
    .unwrap();

    for (state, now) in [
        (unlocked, 1_u64),
        (unarmed, 2),
        (armed, 3),
        (challenging, 4),
        (cancelling, 5),
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
        assert_eq!(effects, vec![Effect::ClearPermit]);
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
        assert_eq!(effects, vec![Effect::ClearPermit]);
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
        assert_eq!(effects, vec![Effect::ClearPermit]);

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
