#[path = "../../repose-unlock-core/tests/common/mod.rs"]
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};
use repose_unlock_core::permit::ConsumeError;
use repose_unlock_core::replay::GenerationAuthorityError;
use repose_unlock_core::replay::{DurableReplayGuard, MemoryCounterStore, ReplayPolicy};
use repose_unlock_core::state_machine::{
    Effect, Event, Permit, SessionBinding, UnlockPhase, UnlockState, transition,
};
use repose_unlock_ipc::{RequestNonce, ServiceInstanceId, SessionSelector};
use repose_unlock_service::permit_broker::{
    BrokerError, BrokerOutcome, MonotonicClock, PermitBroker, WatchWaitError,
};

#[derive(Clone)]
struct ManualClock(Arc<AtomicU64>);

impl ManualClock {
    fn new(now: u64) -> Self {
        Self(Arc::new(AtomicU64::new(now)))
    }

    fn set(&self, now: u64) {
        self.0.store(now, Ordering::SeqCst);
    }
}

impl MonotonicClock for ManualClock {
    fn now(&self) -> MonoMillis {
        MonoMillis::new(self.0.load(Ordering::SeqCst))
    }
}

fn binding() -> SessionBinding {
    common::Vectors::load().binding()
}

fn other_binding() -> SessionBinding {
    SessionBinding::new(
        LockEpoch::new(binding().lock_epoch().get() + 1),
        AuditSessionId::new(binding().audit_session_id().get() + 1),
        ConsoleUid::new(binding().console_uid().get() + 1),
    )
}

fn next_binding() -> SessionBinding {
    SessionBinding::new(
        LockEpoch::new(binding().lock_epoch().get() + 1),
        binding().audit_session_id(),
        binding().console_uid(),
    )
}

fn nonce(value: u8) -> RequestNonce {
    RequestNonce::try_new([value; 32]).unwrap()
}

fn selector() -> SessionSelector {
    SessionSelector::new(binding().console_uid(), binding().audit_session_id())
}

fn instance(value: u8) -> ServiceInstanceId {
    ServiceInstanceId::try_new([value; 16]).unwrap()
}

fn permit_ready(reducer_now: u64) -> (UnlockState, Permit, DurableReplayGuard<MemoryCounterStore>) {
    let (state, response, _) = common::authenticated();
    let guard = DurableReplayGuard::new(MemoryCounterStore::new(), ReplayPolicy::default());
    let committed = guard.commit(response).unwrap();
    let proof = guard.finalize(committed).unwrap();
    let (state, effects) = transition(
        state,
        Event::ChallengeVerified(proof),
        MonoMillis::new(reducer_now),
    )
    .unwrap();
    let [effect]: [Effect; 1] = effects.try_into().unwrap();
    let Effect::CreatePermit(permit) = effect else {
        panic!("expected permit effect");
    };
    (state, permit, guard)
}

#[test]
fn consume_or_watch_registration_cannot_miss_a_publish() {
    let (state, permit, guard) = permit_ready(2_000);
    let clock = ManualClock::new(2_000);
    let broker = PermitBroker::new(guard, clock, instance(1), state, 8);

    let BrokerOutcome::Watching(watch) = broker.consume_or_watch(selector(), nonce(1)).unwrap()
    else {
        panic!("empty broker must register a watch");
    };
    let published = broker.publish(permit).unwrap();
    assert_eq!(published.notified_watchers(), 1);
    let ready = watch.recv_timeout(Duration::from_millis(10)).unwrap();
    assert_eq!(ready.nonce(), nonce(1));
    assert_eq!(ready.binding(), binding());
    assert_eq!(ready.service_instance(), instance(1));
    assert_eq!(ready.watch_id(), watch.watch_id());
    assert_eq!(
        watch.recv_timeout(Duration::from_millis(1)).unwrap_err(),
        WatchWaitError::Closed
    );

    let BrokerOutcome::Authorized(_receipt) =
        broker.consume_or_watch(selector(), nonce(2)).unwrap()
    else {
        panic!("a fresh invocation must atomically consume");
    };
    assert!(broker.is_unlocking());
}

#[test]
fn event_never_consumes_and_expiry_is_rechecked_by_the_second_invocation() {
    let (state, permit, guard) = permit_ready(2_000);
    let clock = ManualClock::new(2_000);
    let broker = PermitBroker::new(guard, clock.clone(), instance(1), state, 8);
    let BrokerOutcome::Watching(watch) = broker.consume_or_watch(selector(), nonce(1)).unwrap()
    else {
        panic!("expected watch");
    };
    broker.publish(permit).unwrap();
    watch.recv_timeout(Duration::from_millis(10)).unwrap();

    clock.set(5_000);
    assert!(matches!(
        broker.consume_or_watch(selector(), nonce(2)).unwrap(),
        BrokerOutcome::Watching(_)
    ));
}

#[test]
fn missing_publish_effect_cannot_leave_an_expired_permit_ready_reducer() {
    let (state, _permit, guard) = permit_ready(2_000);
    let broker = PermitBroker::new(guard, ManualClock::new(5_000), instance(1), state, 8);
    assert!(matches!(
        broker.consume_or_watch(selector(), nonce(1)).unwrap(),
        BrokerOutcome::Watching(_)
    ));
    assert_eq!(broker.phase(), UnlockPhase::LockedArmed);
}

#[test]
fn authorization_receipt_is_an_immutable_old_incarnation_snapshot() {
    let (state, permit, guard) = permit_ready(2_000);
    let broker = PermitBroker::new(guard, ManualClock::new(2_001), instance(1), state, 8);
    broker.publish(permit).unwrap();
    let BrokerOutcome::Authorized(receipt) = broker.consume_or_watch(selector(), nonce(1)).unwrap()
    else {
        panic!("expected authorization receipt");
    };
    broker.restart(instance(2), Some(next_binding())).unwrap();
    assert_eq!(receipt.nonce(), nonce(1));
    assert_eq!(receipt.binding(), binding());
    assert_eq!(receipt.service_instance(), instance(1));
    assert_eq!(receipt.revision(), 1);
}

#[test]
fn restart_changes_instance_clears_permit_and_invalidates_old_watch() {
    let (state, permit, guard) = permit_ready(2_000);
    let clock = ManualClock::new(2_000);
    let broker = PermitBroker::new(guard, clock, instance(1), state, 8);
    let BrokerOutcome::Watching(watch) = broker.consume_or_watch(selector(), nonce(1)).unwrap()
    else {
        panic!("expected watch");
    };
    let old_key = watch.key();
    broker.publish(permit).unwrap();
    broker.restart(instance(2), Some(next_binding())).unwrap();

    assert_eq!(
        watch.recv_timeout(Duration::from_millis(10)).unwrap_err(),
        WatchWaitError::Invalidated
    );
    assert!(!broker.cancel_watch(old_key));
    assert_eq!(broker.service_instance(), instance(2));
    assert!(matches!(
        broker.consume_or_watch(selector(), nonce(2)).unwrap(),
        BrokerOutcome::Watching(_)
    ));
}

#[test]
fn restart_rejects_same_or_lower_epoch_after_clearing_every_capability() {
    for replacement in [
        binding(),
        SessionBinding::new(
            LockEpoch::new(binding().lock_epoch().get() - 1),
            binding().audit_session_id(),
            binding().console_uid(),
        ),
    ] {
        let (state, permit, guard) = permit_ready(2_000);
        let broker = PermitBroker::new(guard, ManualClock::new(2_001), instance(1), state, 8);
        let BrokerOutcome::Watching(watch) = broker.consume_or_watch(selector(), nonce(1)).unwrap()
        else {
            panic!("expected watch");
        };
        broker.publish(permit).unwrap();
        assert_eq!(
            broker.restart(instance(2), Some(replacement)).unwrap_err(),
            BrokerError::RestartEpochNotAdvanced
        );
        assert_eq!(
            watch.recv_timeout(Duration::from_millis(10)).unwrap_err(),
            WatchWaitError::Invalidated
        );
        assert_eq!(broker.service_instance(), instance(2));
        assert_eq!(broker.phase(), UnlockPhase::Unlocked);
        assert_eq!(
            broker.consume_or_watch(selector(), nonce(2)).unwrap_err(),
            BrokerError::SessionMismatch
        );
    }
}

#[test]
fn restart_rejects_reused_instance_after_rotating_and_clearing_capabilities() {
    let (state, permit, guard) = permit_ready(2_000);
    let broker = PermitBroker::new(guard, ManualClock::new(2_001), instance(1), state, 8);
    let BrokerOutcome::Watching(watch) = broker.consume_or_watch(selector(), nonce(1)).unwrap()
    else {
        panic!("expected watch");
    };
    broker.publish(permit).unwrap();

    assert_eq!(
        broker
            .restart(instance(1), Some(next_binding()))
            .unwrap_err(),
        BrokerError::ReusedServiceInstance
    );
    assert_eq!(
        watch.recv_timeout(Duration::from_millis(10)).unwrap_err(),
        WatchWaitError::Invalidated
    );
    assert_ne!(broker.service_instance(), instance(1));
    assert_eq!(broker.phase(), UnlockPhase::Unlocked);
    assert_eq!(broker.watch_count(), 0);
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(2)).unwrap_err(),
        BrokerError::SessionMismatch
    );
}

#[test]
fn session_change_and_clock_rollback_fail_closed_and_close_watchers() {
    let (state, _permit, guard) = permit_ready(2_000);
    let clock = ManualClock::new(2_000);
    let broker = PermitBroker::new(guard, clock.clone(), instance(1), state, 8);
    let BrokerOutcome::Watching(session_watch) =
        broker.consume_or_watch(selector(), nonce(1)).unwrap()
    else {
        panic!("expected watch");
    };
    broker.update_session(Some(other_binding())).unwrap();
    assert_eq!(
        session_watch
            .recv_timeout(Duration::from_millis(10))
            .unwrap_err(),
        WatchWaitError::Invalidated
    );
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(2)).unwrap_err(),
        BrokerError::SessionMismatch
    );

    let replacement = other_binding();
    let replacement_selector =
        SessionSelector::new(replacement.console_uid(), replacement.audit_session_id());
    let BrokerOutcome::Watching(clock_watch) = broker
        .consume_or_watch(replacement_selector, nonce(3))
        .unwrap()
    else {
        panic!("expected watch");
    };
    clock.set(1_999);
    assert_eq!(
        broker
            .consume_or_watch(replacement_selector, nonce(4))
            .unwrap_err(),
        BrokerError::ClockRollback
    );
    assert_eq!(
        clock_watch
            .recv_timeout(Duration::from_millis(10))
            .unwrap_err(),
        WatchWaitError::Invalidated
    );
    assert_ne!(broker.service_instance(), instance(1));
}

#[test]
fn duplicate_nonce_and_watch_limit_are_bounded() {
    let (state, _permit, guard) = permit_ready(2_000);
    let broker = PermitBroker::new(guard, ManualClock::new(2_000), instance(1), state, 2);
    let BrokerOutcome::Watching(_first) = broker.consume_or_watch(selector(), nonce(1)).unwrap()
    else {
        panic!("expected first watch");
    };
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(1)).unwrap_err(),
        BrokerError::DuplicateNonce
    );
    let BrokerOutcome::Watching(_second) = broker.consume_or_watch(selector(), nonce(2)).unwrap()
    else {
        panic!("expected second watch");
    };
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(3)).unwrap_err(),
        BrokerError::WatchLimit
    );
}

#[test]
fn dropping_registration_immediately_cancels_exact_watch() {
    let (state, _permit, guard) = permit_ready(2_000);
    let broker = PermitBroker::new(guard, ManualClock::new(2_000), instance(1), state, 8);
    let BrokerOutcome::Watching(watch) = broker.consume_or_watch(selector(), nonce(1)).unwrap()
    else {
        panic!("expected watch");
    };
    assert_eq!(broker.watch_count(), 1);
    drop(watch);
    assert_eq!(broker.watch_count(), 0);
}

#[test]
fn one_hundred_twenty_nine_pending_watches_stop_at_the_explicit_limit() {
    let (state, _permit, guard) = permit_ready(2_000);
    let broker = PermitBroker::new(guard, ManualClock::new(2_000), instance(1), state, 128);
    let mut watches = Vec::new();
    for value in 1_u8..=128 {
        let mut bytes = [0_u8; 32];
        bytes[0] = value;
        let BrokerOutcome::Watching(watch) = broker
            .consume_or_watch(selector(), RequestNonce::try_new(bytes).unwrap())
            .unwrap()
        else {
            panic!("watch within limit");
        };
        watches.push(watch);
    }
    assert_eq!(broker.watch_count(), 128);
    let mut overflow_nonce = [0_u8; 32];
    overflow_nonce[0] = 129;
    assert_eq!(
        broker
            .consume_or_watch(selector(), RequestNonce::try_new(overflow_nonce).unwrap())
            .unwrap_err(),
        BrokerError::WatchLimit
    );
    drop(watches);
    assert_eq!(broker.watch_count(), 0);
}

#[test]
fn reducer_provenance_mismatch_denies_after_atomic_store_consumption() {
    let (_matching_state, permit, authority) = permit_ready(2_000);
    let (mismatched_state, _other_permit, _other_authority) = permit_ready(2_000);
    let broker = PermitBroker::new(
        authority,
        ManualClock::new(2_001),
        instance(1),
        mismatched_state,
        8,
    );
    broker.publish(permit).unwrap();
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(1)).unwrap_err(),
        BrokerError::AuthorizationRejected
    );
    assert!(!broker.is_unlocking());
}

#[test]
fn revoked_authority_resets_stale_permit_ready_reducer() {
    let (state, response, vectors) = common::authenticated();
    let store = MemoryCounterStore::new();
    let authority = DurableReplayGuard::new(store.clone(), ReplayPolicy::default());
    let committed = authority.commit(response).unwrap();
    let proof = authority.finalize(committed).unwrap();
    let (state, effects) = transition(
        state,
        Event::ChallengeVerified(proof),
        MonoMillis::new(2_000),
    )
    .unwrap();
    let [effect]: [Effect; 1] = effects.try_into().unwrap();
    let Effect::CreatePermit(permit) = effect else {
        panic!("expected permit");
    };
    let revoker = DurableReplayGuard::new(store, ReplayPolicy::default());
    let broker = PermitBroker::new(authority, ManualClock::new(2_001), instance(1), state, 8);
    broker.publish(permit).unwrap();
    revoker
        .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
        .unwrap();
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(1)).unwrap_err(),
        BrokerError::Permit(ConsumeError::Authority(GenerationAuthorityError::Revoked))
    );
    assert_eq!(broker.phase(), UnlockPhase::LockedUnarmed);
    assert_eq!(broker.watch_count(), 0);
}
