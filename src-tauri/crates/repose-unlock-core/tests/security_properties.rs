use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};
use repose_unlock_core::state_machine::{
    ChallengeId, Effect, Event, SessionBinding, UnlockPhase, UnlockState, transition,
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

fn generated_public_event(selector: u64, now: u64) -> Event {
    let current = binding(7, 41, 501);
    let stale_epoch = binding(6, 41, 501);
    let changed_uid = binding(8, 41, 502);
    let changed_audit = binding(8, 42, 501);
    let selected_binding = match selector % 4 {
        0 => current,
        1 => stale_epoch,
        2 => changed_uid,
        _ => changed_audit,
    };
    let challenge_id = ChallengeId::new((selector.rotate_left(17) % 7) + 1);

    match (selector / 4) % 14 {
        0 => Event::SessionLocked {
            binding: selected_binding,
        },
        1 => Event::FarStable {
            binding: selected_binding,
        },
        2 => Event::ReliableDisconnect {
            binding: selected_binding,
        },
        3 => Event::NearStable {
            binding: selected_binding,
            challenge_id,
            deadline: time(now + 40),
        },
        4 => Event::ChallengeFailed {
            binding: selected_binding,
            challenge_id,
            cooldown_until: time(now + 20),
        },
        5 => Event::ChallengeTimedOut {
            binding: selected_binding,
            challenge_id,
            cooldown_until: time(now + 20),
        },
        6 => Event::PermitConsumed {
            binding: selected_binding,
            challenge_id,
        },
        7 => Event::PermitExpired {
            binding: selected_binding,
            challenge_id,
        },
        8 => Event::SessionUnlocked,
        9 => Event::Logout,
        10 => Event::FastUserSwitch {
            locked_binding: Some(selected_binding),
        },
        11 => Event::FastUserSwitch {
            locked_binding: None,
        },
        12 => Event::ServiceRestarted {
            locked_binding: Some(selected_binding),
        },
        _ => Event::ServiceRestarted {
            locked_binding: None,
        },
    }
}

#[test]
fn arbitrary_public_event_sequences_never_create_a_permit_without_verified_proof() {
    for seed in 0_u64..1_024 {
        let mut state = UnlockState::unlocked();
        let mut generator = seed | 1;

        for step in 1_u64..=64 {
            generator ^= generator << 13;
            generator ^= generator >> 7;
            generator ^= generator << 17;
            let event = generated_public_event(generator, step);
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
        let (locked, _) = transition(
            UnlockState::unlocked(),
            Event::SessionLocked { binding: old },
            time(1),
        )
        .unwrap();
        let (armed, _) = transition(locked, Event::FarStable { binding: old }, time(2)).unwrap();
        let (challenging, _) = transition(
            armed,
            Event::NearStable {
                binding: old,
                challenge_id: ChallengeId::new(11),
                deadline: time(40),
            },
            time(3),
        )
        .unwrap();
        let (reset, _) = transition(
            challenging,
            Event::SessionLocked {
                binding: replacement,
            },
            time(4),
        )
        .unwrap();

        let prior_events = [
            Event::FarStable { binding: old },
            Event::NearStable {
                binding: old,
                challenge_id: ChallengeId::new(11),
                deadline: time(40),
            },
            Event::ChallengeFailed {
                binding: old,
                challenge_id: ChallengeId::new(11),
                cooldown_until: time(40),
            },
            Event::ChallengeTimedOut {
                binding: old,
                challenge_id: ChallengeId::new(11),
                cooldown_until: time(40),
            },
            Event::PermitConsumed {
                binding: old,
                challenge_id: ChallengeId::new(11),
            },
            Event::PermitExpired {
                binding: old,
                challenge_id: ChallengeId::new(11),
            },
        ];

        let mut state = reset;
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
    let (locked, _) = transition(
        UnlockState::unlocked(),
        Event::SessionLocked { binding: current },
        time(1),
    )
    .unwrap();
    let (armed, _) = transition(locked, Event::FarStable { binding: current }, time(2)).unwrap();
    let (mut state, effects) = transition(
        armed,
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(1),
            deadline: time(100),
        },
        time(3),
    )
    .unwrap();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| effect.is_start_challenge())
            .count(),
        1
    );

    for step in 4_u64..64 {
        let (next, effects) = transition(
            state,
            Event::NearStable {
                binding: current,
                challenge_id: ChallengeId::new(step),
                deadline: time(100 + step),
            },
            time(step),
        )
        .unwrap();
        assert_eq!(next.phase(), UnlockPhase::Challenging);
        assert_eq!(next.challenge_id(), Some(ChallengeId::new(1)));
        assert_eq!(
            effects
                .iter()
                .filter(|effect| effect.is_start_challenge())
                .count(),
            0
        );
        state = next;
    }
}

#[test]
fn restart_cannot_preserve_any_publicly_reachable_transient_state() {
    let current = binding(7, 41, 501);
    let unlocked = UnlockState::unlocked();
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
    let (challenging, _) = transition(
        armed.clone(),
        Event::NearStable {
            binding: current,
            challenge_id: ChallengeId::new(1),
            deadline: time(50),
        },
        time(3),
    )
    .unwrap();

    for (state, now) in [
        (unlocked, 1_u64),
        (unarmed, 2),
        (armed, 3),
        (challenging, 4),
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
