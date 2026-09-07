mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use parking_lot::Mutex;
use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch};
use repose_unlock_core::permit::{ConsumeError, ExpireOutcome, InstallError, PermitStore};
use repose_unlock_core::protocol::messages::DeviceId;
use repose_unlock_core::replay::{
    CounterStore, DurableCounterState, DurableReplayGuard, DurableReplayState,
    GenerationAuthorityError, MemoryCounterStore, ReplayPolicy, ReplayStoreError,
};
use repose_unlock_core::state_machine::{Effect, Event, Permit, SessionBinding, transition};

use common::{authenticated, ms, permit_at};

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
    let (_, guard) = permit_at(2_000);
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn wrong_request_binding_retains_permit_for_correct_consumer() {
    let store = PermitStore::new();
    let (permit, guard) = permit_at(2_000);
    store.install(permit, ms(2_000)).unwrap();
    assert_eq!(
        store
            .consume(&guard, other_binding(), Some(binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::RequestBindingMismatch
    );
    let consumed = store
        .consume(&guard, binding(), Some(binding()), ms(2_002))
        .unwrap();
    assert_eq!(consumed.binding(), binding());
    assert_eq!(consumed.challenge_id().get(), 1);
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(2_003))
            .unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn exact_expiry_boundary_destroys_permit() {
    let store = PermitStore::new();
    let (permit, guard) = permit_at(2_000);
    store.install(permit, ms(2_000)).unwrap();
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(5_000))
            .unwrap_err(),
        ConsumeError::Expired
    );
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(5_001))
            .unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn authoritative_session_mismatch_destroys_permit() {
    let store = PermitStore::new();
    let (permit, guard) = permit_at(2_000);
    store.install(permit, ms(2_000)).unwrap();
    assert_eq!(
        store
            .consume(&guard, binding(), Some(other_binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::AuthoritativeSessionMismatch
    );
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(2_002))
            .unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn replace_clear_restart_and_stale_cleanup_have_linear_semantics() {
    let store = PermitStore::new();
    let (first_permit, _first_guard) = permit_at(2_000);
    let first = store.install(first_permit, ms(2_000)).unwrap();
    let (second_permit, second_guard) = permit_at(2_100);
    let second = store.install(second_permit, ms(2_100)).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        store.expire_if(first, Some(binding()), ms(5_000)).unwrap(),
        ExpireOutcome::StaleHandle
    );
    assert!(!store.clear_if(first));
    assert!(
        store
            .consume(&second_guard, binding(), Some(binding()), ms(2_101))
            .is_ok()
    );

    let (permit, guard) = permit_at(2_200);
    store.install(permit, ms(2_200)).unwrap();
    store.clear();
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(2_201))
            .unwrap_err(),
        ConsumeError::Empty
    );

    let (permit, guard) = permit_at(2_250);
    store.install(permit, ms(2_250)).unwrap();
    store.revoke();
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(2_251))
            .unwrap_err(),
        ConsumeError::Empty
    );

    let (permit, guard) = permit_at(2_300);
    let before_restart = store.install(permit, ms(2_300)).unwrap();
    store.service_restarted();
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(1))
            .unwrap_err(),
        ConsumeError::Empty
    );
    let (permit, _) = permit_at(2_400);
    let after_restart = store.install(permit, ms(2_400)).unwrap();
    assert_ne!(
        before_restart, after_restart,
        "permit handles are never reused"
    );
}

#[test]
fn one_hundred_concurrent_consumers_get_exactly_one_success() {
    let store = Arc::new(PermitStore::new());
    let (permit, guard) = permit_at(2_000);
    let guard = Arc::new(guard);
    store.install(permit, ms(2_000)).unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let workers: Vec<_> = (0..100)
        .map(|_| {
            let store = Arc::clone(&store);
            let guard = Arc::clone(&guard);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store
                    .consume(&guard, binding(), Some(binding()), ms(2_001))
                    .is_ok()
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
    let (permit, guard) = permit_at(2_000);
    let guard = Arc::new(guard);
    store.install(permit, ms(2_000)).unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let workers: Vec<_> = (0..100)
        .map(|index| {
            let store = Arc::clone(&store);
            let guard = Arc::clone(&guard);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let requested = if index == 0 {
                    binding()
                } else {
                    other_binding()
                };
                store
                    .consume(&guard, requested, Some(binding()), ms(2_001))
                    .is_ok()
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
    let (old_permit, _) = permit_at(2_000);
    let old = store.install(old_permit, ms(2_000)).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let expiry_worker = {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            store.expire_if(old, Some(binding()), ms(5_000))
        })
    };
    let replace_worker = {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            let (permit, guard) = permit_at(5_500);
            store
                .install(permit, ms(5_500))
                .map(|handle| (handle, guard))
        })
    };
    expiry_worker.join().unwrap().unwrap();
    let (_, guard) = replace_worker.join().unwrap().unwrap();
    assert!(
        store
            .consume(&guard, binding(), Some(binding()), ms(5_501))
            .is_ok()
    );
}

#[test]
fn invalid_lifetime_and_nonmonotonic_install_are_rejected() {
    let store = PermitStore::new();
    let (permit, _) = permit_at(2_000);
    assert_eq!(
        store.install(permit, ms(5_000)).unwrap_err(),
        InstallError::InvalidLifetime
    );

    let (permit, _) = permit_at(2_000);
    store.install(permit, ms(2_000)).unwrap();
    store.clear();
    assert_eq!(
        store.install(permit_at(2_000).0, ms(1_999)).unwrap_err(),
        InstallError::NonMonotonicTime
    );
}

#[test]
fn time_rollback_during_consume_destroys_the_permit() {
    let store = PermitStore::new();
    let (permit, guard) = permit_at(2_000);
    store.install(permit, ms(2_000)).unwrap();
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(1_999))
            .unwrap_err(),
        ConsumeError::NonMonotonicTime
    );
    assert_eq!(
        store
            .consume(&guard, binding(), Some(binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::Empty
    );
}

fn provenance_permit<S: CounterStore>(guard: &DurableReplayGuard<S>) -> (Permit, common::Vectors) {
    let (state, response, vectors) = authenticated();
    let committed = guard.commit(response).unwrap();
    let proof = guard.finalize(committed).unwrap();
    let (_, effects) = transition(state, Event::ChallengeVerified(proof), ms(2_000)).unwrap();
    let [effect]: [Effect; 1] = effects.try_into().unwrap();
    let Effect::CreatePermit(permit) = effect else {
        panic!("expected permit")
    };
    (permit, vectors)
}

#[test]
fn durable_revoke_after_finalize_prevents_permit_consumption_and_destroys_it() {
    let replay_store = MemoryCounterStore::new();
    let guard = DurableReplayGuard::new(replay_store, ReplayPolicy::default());
    let (permit, vectors) = provenance_permit(&guard);
    let permits = PermitStore::new();
    permits.install(permit, ms(2_000)).unwrap();
    guard
        .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
        .unwrap();

    assert_eq!(
        permits
            .consume(&guard, binding(), Some(binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::Authority(GenerationAuthorityError::Revoked)
    );
    assert_eq!(
        permits
            .consume(&guard, binding(), Some(binding()), ms(2_002))
            .unwrap_err(),
        ConsumeError::Empty
    );
}

#[derive(Clone)]
struct ToggleStore {
    backing: MemoryCounterStore,
    unavailable: Arc<AtomicBool>,
}

impl CounterStore for ToggleStore {
    fn load(&self, device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        if self.unavailable.load(Ordering::SeqCst) {
            Err(ReplayStoreError::Unavailable)
        } else {
            self.backing.load(device_id)
        }
    }

    fn compare_and_swap(
        &self,
        device_id: DeviceId,
        expected: Option<&DurableReplayState>,
        replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        if self.unavailable.load(Ordering::SeqCst) {
            Err(ReplayStoreError::Unavailable)
        } else {
            self.backing
                .compare_and_swap(device_id, expected, replacement)
        }
    }
}

#[test]
fn authority_store_failure_returns_no_capability_and_destroys_the_permit() {
    let unavailable = Arc::new(AtomicBool::new(false));
    let replay_store = ToggleStore {
        backing: MemoryCounterStore::new(),
        unavailable: Arc::clone(&unavailable),
    };
    let guard = DurableReplayGuard::new(replay_store, ReplayPolicy::default());
    let (permit, _) = provenance_permit(&guard);
    let permits = PermitStore::new();
    permits.install(permit, ms(2_000)).unwrap();
    unavailable.store(true, Ordering::SeqCst);

    assert_eq!(
        permits
            .consume(&guard, binding(), Some(binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::Authority(GenerationAuthorityError::Store(
            ReplayStoreError::Unavailable
        ))
    );
    unavailable.store(false, Ordering::SeqCst);
    assert_eq!(
        permits
            .consume(&guard, binding(), Some(binding()), ms(2_002))
            .unwrap_err(),
        ConsumeError::Empty
    );
}

#[derive(Clone)]
struct StaleReadAfterRevokeStore {
    backing: MemoryCounterStore,
    stale_read: Arc<Mutex<Option<DurableReplayState>>>,
}

impl CounterStore for StaleReadAfterRevokeStore {
    fn load(&self, device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        if let Some(stale) = *self.stale_read.lock() {
            Ok(Some(stale))
        } else {
            self.backing.load(device_id)
        }
    }

    fn compare_and_swap(
        &self,
        device_id: DeviceId,
        expected: Option<&DurableReplayState>,
        replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        self.backing
            .compare_and_swap(device_id, expected, replacement)
    }
}

#[test]
fn stale_read_cannot_authorize_a_permit_after_revocation_has_returned() {
    let store = StaleReadAfterRevokeStore {
        backing: MemoryCounterStore::new(),
        stale_read: Arc::new(Mutex::new(None)),
    };
    let guard = DurableReplayGuard::new(store.clone(), ReplayPolicy::default());
    let (permit, vectors) = provenance_permit(&guard);
    let active = store.backing.load(vectors.device_id()).unwrap().unwrap();
    let permits = PermitStore::new();
    permits.install(permit, ms(2_000)).unwrap();
    guard
        .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
        .unwrap();
    *store.stale_read.lock() = Some(active);

    assert_eq!(
        permits
            .consume(&guard, binding(), Some(binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::Authority(GenerationAuthorityError::StateMismatch)
    );
    *store.stale_read.lock() = None;
    assert_eq!(
        permits
            .consume(&guard, binding(), Some(binding()), ms(2_002))
            .unwrap_err(),
        ConsumeError::Empty
    );
}

#[test]
fn wrong_guard_or_changed_durable_commit_cannot_validate_a_permit() {
    let replay_store = MemoryCounterStore::new();
    let guard = DurableReplayGuard::new(replay_store.clone(), ReplayPolicy::default());
    let (permit, vectors) = provenance_permit(&guard);
    let permits = PermitStore::new();
    permits.install(permit, ms(2_000)).unwrap();
    let other_guard = DurableReplayGuard::new(replay_store.clone(), ReplayPolicy::default());
    assert_eq!(
        permits
            .consume(&other_guard, binding(), Some(binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::Authority(GenerationAuthorityError::WrongGuard)
    );

    let replay_store = MemoryCounterStore::new();
    let guard = DurableReplayGuard::new(replay_store.clone(), ReplayPolicy::default());
    let (permit, _) = provenance_permit(&guard);
    permits.install(permit, ms(2_002)).unwrap();
    let snapshot = replay_store.snapshot();
    let [DurableReplayState::Active(active)] = snapshot.entries() else {
        panic!("active durable response")
    };
    let expected = DurableReplayState::Active(*active);
    let replacement = DurableReplayState::Active(
        DurableCounterState::new(
            active.mac_id(),
            active.device_id(),
            active.pairing_generation(),
            active.binding(),
            active.challenge_id() + 1,
            active.counter() + 1,
        )
        .unwrap(),
    );
    assert!(
        replay_store
            .compare_and_swap(vectors.device_id(), Some(&expected), &replacement,)
            .unwrap()
    );
    assert_eq!(
        permits
            .consume(&guard, binding(), Some(binding()), ms(2_003))
            .unwrap_err(),
        ConsumeError::Authority(GenerationAuthorityError::StateMismatch)
    );
}

#[derive(Clone)]
struct FinalizeBarrierStore {
    backing: MemoryCounterStore,
    loads: Arc<AtomicUsize>,
    readback_observed: Arc<Barrier>,
    release_readback: Arc<Barrier>,
}

impl CounterStore for FinalizeBarrierStore {
    fn load(&self, device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        let result = self.backing.load(device_id)?;
        if self.loads.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
            self.readback_observed.wait();
            self.release_readback.wait();
        }
        Ok(result)
    }

    fn compare_and_swap(
        &self,
        device_id: DeviceId,
        expected: Option<&DurableReplayState>,
        replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        self.backing
            .compare_and_swap(device_id, expected, replacement)
    }
}

#[test]
fn revoke_between_finalize_readback_and_proof_delivery_still_fences_consumption() {
    let (state, response, vectors) = authenticated();
    let readback_observed = Arc::new(Barrier::new(2));
    let release_readback = Arc::new(Barrier::new(2));
    let store = FinalizeBarrierStore {
        backing: MemoryCounterStore::new(),
        loads: Arc::new(AtomicUsize::new(0)),
        readback_observed: Arc::clone(&readback_observed),
        release_readback: Arc::clone(&release_readback),
    };
    let guard = Arc::new(DurableReplayGuard::new(store, ReplayPolicy::default()));
    let committed = guard.commit(response).unwrap();
    let finalize_guard = Arc::clone(&guard);
    let worker = thread::spawn(move || finalize_guard.finalize(committed));

    readback_observed.wait();
    guard
        .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
        .unwrap();
    release_readback.wait();
    let proof = worker.join().unwrap().unwrap();
    let (_, effects) = transition(state, Event::ChallengeVerified(proof), ms(2_000)).unwrap();
    let [effect]: [Effect; 1] = effects.try_into().unwrap();
    let Effect::CreatePermit(permit) = effect else {
        panic!("expected permit")
    };
    let permits = PermitStore::new();
    permits.install(permit, ms(2_000)).unwrap();

    assert_eq!(
        permits
            .consume(&guard, binding(), Some(binding()), ms(2_001))
            .unwrap_err(),
        ConsumeError::Authority(GenerationAuthorityError::Revoked)
    );
}

#[test]
fn stale_expiry_handle_still_clears_a_replacement_invalid_for_current_session() {
    let permits = PermitStore::new();
    let (old_permit, _) = permit_at(2_000);
    let old_handle = permits.install(old_permit, ms(2_000)).unwrap();
    let (replacement, replacement_guard) = permit_at(2_100);
    permits.install(replacement, ms(2_100)).unwrap();

    assert_eq!(
        permits
            .expire_if(old_handle, Some(other_binding()), ms(2_101))
            .unwrap(),
        ExpireOutcome::AuthoritativeSessionMismatch
    );
    assert_eq!(
        permits
            .consume(&replacement_guard, binding(), Some(binding()), ms(2_102),)
            .unwrap_err(),
        ConsumeError::Empty
    );

    let (old_permit, _) = permit_at(2_200);
    let old_handle = permits.install(old_permit, ms(2_200)).unwrap();
    let (replacement, replacement_guard) = permit_at(2_300);
    permits.install(replacement, ms(2_300)).unwrap();
    assert_eq!(
        permits.expire_if(old_handle, None, ms(2_301)).unwrap(),
        ExpireOutcome::AuthoritativeSessionMismatch
    );
    assert_eq!(
        permits
            .consume(&replacement_guard, binding(), Some(binding()), ms(2_302),)
            .unwrap_err(),
        ConsumeError::Empty
    );
}
