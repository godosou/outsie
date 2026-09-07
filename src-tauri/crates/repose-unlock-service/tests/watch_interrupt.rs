#[path = "../../repose-unlock-core/tests/common/mod.rs"]
mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
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

/// Pauses exactly one operation during its in-lock clock read. The two-phase
/// barrier proves that operation owns the broker linearization domain before a
/// competing worker is released, without scheduler sleeps.
#[derive(Clone)]
struct ControlledClock {
    now: Arc<AtomicU64>,
    armed: Arc<std::sync::atomic::AtomicBool>,
    rendezvous: Arc<Barrier>,
}

impl ControlledClock {
    fn new(now: u64) -> Self {
        Self {
            now: Arc::new(AtomicU64::new(now)),
            armed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rendezvous: Arc::new(Barrier::new(2)),
        }
    }

    fn arm(&self) {
        assert!(
            !self.armed.swap(true, Ordering::AcqRel),
            "only one broker operation may be gated"
        );
    }

    fn wait_until_blocked(&self) {
        self.rendezvous.wait();
    }

    fn release(&self) {
        self.rendezvous.wait();
    }
}

impl MonotonicClock for ControlledClock {
    fn now(&self) -> MonoMillis {
        if self.armed.swap(false, Ordering::AcqRel) {
            self.rendezvous.wait();
            self.rendezvous.wait();
        }
        MonoMillis::new(self.now.load(Ordering::Acquire))
    }
}

fn linearize_first<A, B, F, G>(clock: &ControlledClock, first: F, contender: G) -> (A, B)
where
    A: Send + 'static,
    B: Send + 'static,
    F: FnOnce() -> A + Send + 'static,
    G: FnOnce() -> B + Send + 'static,
{
    clock.arm();
    let first = thread::spawn(first);
    clock.wait_until_blocked();
    let boundary = Arc::new(Barrier::new(2));
    let thread_boundary = Arc::clone(&boundary);
    let contender = thread::spawn(move || {
        thread_boundary.wait();
        contender()
    });
    boundary.wait();
    clock.release();
    (first.join().unwrap(), contender.join().unwrap())
}

type ControlledBroker = PermitBroker<MemoryCounterStore, ControlledClock>;

fn controlled_ready_broker() -> (Arc<ControlledBroker>, ControlledClock, Permit) {
    let (state, permit, guard) = permit_ready(2_000);
    let clock = ControlledClock::new(2_001);
    let broker = Arc::new(PermitBroker::new(
        guard,
        clock.clone(),
        instance(1),
        state,
        8,
    ));
    (broker, clock, permit)
}

fn controlled_published_broker() -> (Arc<ControlledBroker>, ControlledClock) {
    let (broker, clock, permit) = controlled_ready_broker();
    broker.publish(permit).unwrap();
    (broker, clock)
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

#[test]
fn publish_and_consume_or_watch_follow_both_controlled_linearization_orders() {
    // History A: consume-or-watch owns the broker lock while the publisher is
    // released at its call boundary. It must register exactly one watch, which
    // the later publish wakes exactly once without consuming the permit.
    let (broker, clock, permit) = controlled_ready_broker();
    let consumer_broker = Arc::clone(&broker);
    let publisher_broker = Arc::clone(&broker);
    let (consumer, publisher) = linearize_first(
        &clock,
        move || consumer_broker.consume_or_watch(selector(), nonce(1)),
        move || publisher_broker.publish(permit),
    );
    let BrokerOutcome::Watching(watch) = consumer.unwrap() else {
        panic!("consume-first history must register a watch");
    };
    assert_eq!(publisher.unwrap().notified_watchers(), 1);
    let ready = watch.recv_timeout(Duration::ZERO).unwrap();
    assert_eq!(ready.nonce(), watch.nonce());
    assert_eq!(ready.binding(), watch.binding());
    assert_eq!(ready.service_instance(), watch.service_instance());
    assert_eq!(ready.watch_id(), watch.watch_id());
    assert_eq!(
        watch.recv_timeout(Duration::ZERO).unwrap_err(),
        WatchWaitError::Closed
    );
    assert!(matches!(
        broker.consume_or_watch(selector(), nonce(2)),
        Ok(BrokerOutcome::Authorized(_))
    ));
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(3)).unwrap_err(),
        BrokerError::NotAuthorizable
    );
    assert_eq!(broker.watch_count(), 0);

    // History B: publish owns the broker lock when the consumer is released.
    // The invocation consumes immediately and no watch/event exists.
    let (broker, clock, permit) = controlled_ready_broker();
    let publisher_broker = Arc::clone(&broker);
    let consumer_broker = Arc::clone(&broker);
    let (publisher, consumer) = linearize_first(
        &clock,
        move || publisher_broker.publish(permit),
        move || consumer_broker.consume_or_watch(selector(), nonce(4)),
    );
    assert_eq!(publisher.unwrap().notified_watchers(), 0);
    assert!(matches!(consumer, Ok(BrokerOutcome::Authorized(_))));
    assert_eq!(broker.watch_count(), 0);
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(5)).unwrap_err(),
        BrokerError::NotAuthorizable
    );
}

#[test]
fn session_change_and_consume_or_watch_have_controlled_fail_closed_orders() {
    // Session change wins: the old-session invocation cannot consume the old
    // permit or register a watch under the replacement binding.
    let (broker, clock) = controlled_published_broker();
    let session_broker = Arc::clone(&broker);
    let consumer_broker = Arc::clone(&broker);
    let (session_change, old_consumer) = linearize_first(
        &clock,
        move || session_broker.update_session(Some(other_binding())),
        move || consumer_broker.consume_or_watch(selector(), nonce(1)),
    );
    session_change.unwrap();
    assert_eq!(old_consumer.unwrap_err(), BrokerError::SessionMismatch);
    assert!(!broker.is_unlocking());
    assert_eq!(broker.watch_count(), 0);
    let replacement_selector = SessionSelector::new(
        other_binding().console_uid(),
        other_binding().audit_session_id(),
    );
    let BrokerOutcome::Watching(replacement_watch) = broker
        .consume_or_watch(replacement_selector, nonce(2))
        .unwrap()
    else {
        panic!("replacement session may only receive a non-authorizing watch");
    };
    assert_eq!(replacement_watch.binding(), other_binding());
    drop(replacement_watch);
    assert_eq!(broker.watch_count(), 0);

    // Consume wins: its immutable receipt keeps the old binding/instance even
    // though the overlapping session change completes before the caller joins.
    let (broker, clock) = controlled_published_broker();
    let consumer_broker = Arc::clone(&broker);
    let session_broker = Arc::clone(&broker);
    let (consumer, session_change) = linearize_first(
        &clock,
        move || consumer_broker.consume_or_watch(selector(), nonce(3)),
        move || session_broker.update_session(Some(other_binding())),
    );
    let BrokerOutcome::Authorized(receipt) = consumer.unwrap() else {
        panic!("consume-first history must authorize exactly once");
    };
    session_change.unwrap();
    assert_eq!(receipt.nonce(), nonce(3));
    assert_eq!(receipt.binding(), binding());
    assert_eq!(receipt.service_instance(), instance(1));
    assert_eq!(
        broker.consume_or_watch(selector(), nonce(4)).unwrap_err(),
        BrokerError::SessionMismatch
    );
    assert_eq!(broker.watch_count(), 0);
}

#[test]
fn restart_and_consume_or_watch_never_relabel_old_capabilities() {
    // Restart wins: the old permit is gone, and the overlapping invocation can
    // only register against the new full binding and service instance.
    let (broker, clock) = controlled_published_broker();
    let restart_broker = Arc::clone(&broker);
    let consumer_broker = Arc::clone(&broker);
    let (restart, old_consumer) = linearize_first(
        &clock,
        move || restart_broker.restart(instance(2), Some(next_binding())),
        move || consumer_broker.consume_or_watch(selector(), nonce(1)),
    );
    restart.unwrap();
    let BrokerOutcome::Watching(new_watch) = old_consumer.unwrap() else {
        panic!("restart-first history must not authorize the old permit");
    };
    assert_eq!(new_watch.binding(), next_binding());
    assert_eq!(new_watch.service_instance(), instance(2));
    assert_eq!(
        new_watch.recv_timeout(Duration::ZERO).unwrap_err(),
        WatchWaitError::Timeout
    );
    drop(new_watch);
    assert_eq!(broker.watch_count(), 0);

    // Consume wins: restart cannot relabel the already returned receipt. A
    // subsequent invocation sees only the new generation and gets a new watch.
    let (broker, clock) = controlled_published_broker();
    let consumer_broker = Arc::clone(&broker);
    let restart_broker = Arc::clone(&broker);
    let (consumer, restart) = linearize_first(
        &clock,
        move || consumer_broker.consume_or_watch(selector(), nonce(2)),
        move || restart_broker.restart(instance(2), Some(next_binding())),
    );
    let BrokerOutcome::Authorized(receipt) = consumer.unwrap() else {
        panic!("consume-first history must authorize exactly once");
    };
    restart.unwrap();
    assert_eq!(receipt.nonce(), nonce(2));
    assert_eq!(receipt.binding(), binding());
    assert_eq!(receipt.service_instance(), instance(1));
    let BrokerOutcome::Watching(new_watch) = broker.consume_or_watch(selector(), nonce(3)).unwrap()
    else {
        panic!("post-restart invocation must not authorize the old permit");
    };
    assert_eq!(new_watch.binding(), next_binding());
    assert_eq!(new_watch.service_instance(), instance(2));
    drop(new_watch);
    assert_eq!(broker.watch_count(), 0);
}
