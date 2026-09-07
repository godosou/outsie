mod common;

use std::sync::{Arc, Barrier};
use std::thread;

use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch};
use repose_unlock_core::permit::{ConsumeError, ExpireOutcome, InstallError, PermitStore};
use repose_unlock_core::state_machine::SessionBinding;

use common::{ms, permit_at};

fn binding() -> SessionBinding {
    SessionBinding::new(
        LockEpoch::new(0x0102_0304_0506_0708),
        AuditSessionId::new(0x1234_5678),
        ConsoleUid::new(501),
    )
}

fn other_binding() -> SessionBinding {
    SessionBinding::new(
        LockEpoch::new(0x0102_0304_0506_0709),
        AuditSessionId::new(0x1234_5679),
        ConsoleUid::new(502),
    )
}

#[test]
fn empty_store_fails_closed() {
    let store = PermitStore::new();
    assert_eq!(
        store.consume(binding(), binding(), ms(2_001)).unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn wrong_request_binding_retains_permit_for_correct_consumer() {
    let store = PermitStore::new();
    store.install(permit_at(2_000), ms(2_000)).unwrap();
    assert_eq!(
        store
            .consume(other_binding(), binding(), ms(2_001))
            .unwrap_err(),
        ConsumeError::RequestBindingMismatch
    );
    let consumed = store.consume(binding(), binding(), ms(2_002)).unwrap();
    assert_eq!(consumed.binding(), binding());
    assert_eq!(consumed.challenge_id().get(), 1);
    assert_eq!(
        store.consume(binding(), binding(), ms(2_003)).unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn exact_expiry_boundary_destroys_permit() {
    let store = PermitStore::new();
    store.install(permit_at(2_000), ms(2_000)).unwrap();
    assert_eq!(
        store.consume(binding(), binding(), ms(5_000)).unwrap_err(),
        ConsumeError::Expired
    );
    assert_eq!(
        store.consume(binding(), binding(), ms(5_001)).unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn authoritative_session_mismatch_destroys_permit() {
    let store = PermitStore::new();
    store.install(permit_at(2_000), ms(2_000)).unwrap();
    assert_eq!(
        store
            .consume(binding(), other_binding(), ms(2_001))
            .unwrap_err(),
        ConsumeError::AuthoritativeSessionMismatch
    );
    assert_eq!(
        store.consume(binding(), binding(), ms(2_002)).unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn replace_clear_restart_and_stale_cleanup_have_linear_semantics() {
    let store = PermitStore::new();
    let first = store.install(permit_at(2_000), ms(2_000)).unwrap();
    let second = store.install(permit_at(2_100), ms(2_100)).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        store.expire_if(first, binding(), ms(5_000)).unwrap(),
        ExpireOutcome::StaleHandle
    );
    assert!(!store.clear_if(first));
    assert!(store.consume(binding(), binding(), ms(2_101)).is_ok());

    store.install(permit_at(2_200), ms(2_200)).unwrap();
    store.clear();
    assert_eq!(
        store.consume(binding(), binding(), ms(2_201)).unwrap_err(),
        ConsumeError::Empty
    );

    store.install(permit_at(2_250), ms(2_250)).unwrap();
    store.revoke();
    assert_eq!(
        store.consume(binding(), binding(), ms(2_251)).unwrap_err(),
        ConsumeError::Empty
    );

    let before_restart = store.install(permit_at(2_300), ms(2_300)).unwrap();
    store.service_restarted();
    assert_eq!(
        store.consume(binding(), binding(), ms(1)).unwrap_err(),
        ConsumeError::Empty
    );
    let after_restart = store.install(permit_at(2_400), ms(2_400)).unwrap();
    assert_ne!(
        before_restart, after_restart,
        "permit handles are never reused"
    );
}

#[test]
fn one_hundred_concurrent_consumers_get_exactly_one_success() {
    let store = Arc::new(PermitStore::new());
    store.install(permit_at(2_000), ms(2_000)).unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let workers: Vec<_> = (0..100)
        .map(|_| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store.consume(binding(), binding(), ms(2_001)).is_ok()
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|success| *success)
            .count(),
        1
    );
}

#[test]
fn wrong_binding_race_cannot_destroy_the_correct_consumers_permit() {
    let store = Arc::new(PermitStore::new());
    store.install(permit_at(2_000), ms(2_000)).unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let workers: Vec<_> = (0..100)
        .map(|index| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let requested = if index == 0 {
                    binding()
                } else {
                    other_binding()
                };
                store.consume(requested, binding(), ms(2_001)).is_ok()
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|success| *success)
            .count(),
        1
    );
}

#[test]
fn old_expiry_racing_replacement_never_deletes_the_new_permit() {
    let store = Arc::new(PermitStore::new());
    let old = store.install(permit_at(2_000), ms(2_000)).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let expiry_worker = {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            store.expire_if(old, binding(), ms(5_000))
        })
    };
    let replace_worker = {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            store.install(permit_at(5_500), ms(5_500))
        })
    };
    expiry_worker.join().unwrap().unwrap();
    replace_worker.join().unwrap().unwrap();
    assert!(store.consume(binding(), binding(), ms(5_501)).is_ok());
}

#[test]
fn invalid_lifetime_and_nonmonotonic_install_are_rejected() {
    let store = PermitStore::new();
    assert_eq!(
        store.install(permit_at(2_000), ms(5_000)).unwrap_err(),
        InstallError::InvalidLifetime
    );

    store.install(permit_at(2_000), ms(2_000)).unwrap();
    store.clear();
    assert_eq!(
        store.install(permit_at(2_000), ms(1_999)).unwrap_err(),
        InstallError::NonMonotonicTime
    );
}

#[test]
fn time_rollback_during_consume_destroys_the_permit() {
    let store = PermitStore::new();
    store.install(permit_at(2_000), ms(2_000)).unwrap();
    assert_eq!(
        store.consume(binding(), binding(), ms(1_999)).unwrap_err(),
        ConsumeError::NonMonotonicTime
    );
    assert_eq!(
        store.consume(binding(), binding(), ms(2_001)).unwrap_err(),
        ConsumeError::Empty
    );
}
