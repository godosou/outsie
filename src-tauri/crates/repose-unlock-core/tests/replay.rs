mod common;

use std::sync::{Arc, Barrier};
use std::thread;

use repose_unlock_core::protocol::messages::{DeviceId, MacId, PairingGeneration};
use repose_unlock_core::replay::{
    CounterStore, DurableCounterState, DurableReplayGuard, DurableReplayState, MemoryCounterStore,
    ReplayError, ReplayPolicy, ReplayStoreError, RevokeOutcome,
};
use repose_unlock_core::state_machine::SessionBinding;

use common::{authenticated, authenticated_with_counter, authenticated_with_generation};

#[test]
fn absent_state_means_zero_and_a_valid_large_non_unit_increment_commits() {
    let (_, response, _) = authenticated();
    let store = MemoryCounterStore::new();
    let guard = DurableReplayGuard::new(store.clone(), ReplayPolicy::default());
    let committed = guard.commit(response).unwrap();
    assert_eq!(committed.counter(), 42);
    assert_eq!(store.snapshot().entries().len(), 1);
    assert!(matches!(
        store.snapshot().entries(),
        [DurableReplayState::Active(value)] if value.counter() == 42
    ));
}

#[test]
fn equal_and_lower_counters_never_produce_a_commit_receipt() {
    for durable_counter in [42, 43] {
        let (_, response, vectors) = authenticated();
        let durable = DurableCounterState::new(
            vectors.mac_id(),
            vectors.device_id(),
            vectors.generation(),
            vectors.binding(),
            0,
            durable_counter,
        )
        .unwrap();
        let store = MemoryCounterStore::from_entries([durable]).unwrap();
        let guard = DurableReplayGuard::new(store, ReplayPolicy::default());
        assert_eq!(
            guard.commit(response).unwrap_err(),
            ReplayError::NotAdvanced
        );
    }
}

#[test]
fn jump_policy_is_explicit_and_checked_without_overflow() {
    let (_, response, _) = authenticated();
    let guard = DurableReplayGuard::new(MemoryCounterStore::new(), ReplayPolicy::new(41).unwrap());
    assert_eq!(
        guard.commit(response).unwrap_err(),
        ReplayError::JumpTooLarge {
            previous: 0,
            proposed: 42,
            maximum: 41,
        }
    );
    assert!(ReplayPolicy::new(0).is_err());
}

#[test]
fn maximum_counter_can_commit_once_and_can_never_advance_or_wrap() {
    let (_, maximum, _) = authenticated_with_counter(u64::MAX);
    let store = MemoryCounterStore::new();
    let guard = DurableReplayGuard::new(store, ReplayPolicy::new(u64::MAX).unwrap());
    assert_eq!(guard.commit(maximum).unwrap().counter(), u64::MAX);

    let (_, repeated, _) = authenticated_with_counter(u64::MAX);
    assert_eq!(
        guard.commit(repeated).unwrap_err(),
        ReplayError::NotAdvanced
    );
}

#[test]
fn one_hundred_same_counter_commits_have_exactly_one_winner() {
    let responses: Vec<_> = (0..100).map(|_| authenticated().1).collect();
    let guard = Arc::new(DurableReplayGuard::new(
        MemoryCounterStore::new(),
        ReplayPolicy::default(),
    ));
    let barrier = Arc::new(Barrier::new(responses.len()));
    let threads: Vec<_> = responses
        .into_iter()
        .map(|response| {
            let guard = Arc::clone(&guard);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                guard
                    .commit(response)
                    .and_then(|committed| guard.finalize(committed))
                    .is_ok()
            })
        })
        .collect();
    let successes = threads
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .filter(|success| *success)
        .count();
    assert_eq!(successes, 1);
}

#[test]
fn two_different_valid_counters_for_one_challenge_still_yield_one_proof() {
    let response_42 = authenticated_with_counter(42).1;
    let response_43 = authenticated_with_counter(43).1;
    let guard = Arc::new(DurableReplayGuard::new(
        MemoryCounterStore::new(),
        ReplayPolicy::default(),
    ));
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = [response_42, response_43]
        .into_iter()
        .map(|response| {
            let guard = Arc::clone(&guard);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                guard
                    .commit(response)
                    .and_then(|committed| guard.finalize(committed))
                    .is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|success| *success)
            .count(),
        1
    );
}

#[test]
fn storage_error_and_failed_cas_return_no_committed_capability() {
    let (_, response, _) = authenticated();
    let guard = DurableReplayGuard::new(FailingStore, ReplayPolicy::default());
    assert_eq!(
        guard.commit(response).unwrap_err(),
        ReplayError::Store(ReplayStoreError::Unavailable)
    );

    let (_, response, _) = authenticated();
    let guard = DurableReplayGuard::new(AlwaysContendedStore, ReplayPolicy::default());
    assert_eq!(
        guard.commit(response).unwrap_err(),
        ReplayError::ConcurrentUpdate
    );
}

#[test]
fn mismatched_device_or_pairing_snapshot_fails_closed() {
    let (_, response, vectors) = authenticated();
    let wrong = DurableCounterState::new(
        vectors.mac_id(),
        DeviceId::new([0x99; 16]),
        vectors.generation(),
        vectors.binding(),
        0,
        1,
    )
    .unwrap();
    let guard = DurableReplayGuard::new(ReturnedStateStore(wrong), ReplayPolicy::default());
    assert_eq!(
        guard.commit(response).unwrap_err(),
        ReplayError::CorruptSnapshot
    );

    let (_, response, vectors) = authenticated();
    let wrong_generation = DurableCounterState::new(
        vectors.mac_id(),
        vectors.device_id(),
        PairingGeneration::new(vectors.generation().get() + 1),
        vectors.binding(),
        0,
        1,
    )
    .unwrap();
    let guard = DurableReplayGuard::new(
        ReturnedStateStore(wrong_generation),
        ReplayPolicy::default(),
    );
    assert_eq!(
        guard.commit(response).unwrap_err(),
        ReplayError::StaleGeneration
    );
}

#[test]
fn durable_snapshot_rejects_replay_after_service_restart() {
    let (_, first, _) = authenticated();
    let store = MemoryCounterStore::new();
    DurableReplayGuard::new(store.clone(), ReplayPolicy::default())
        .commit(first)
        .unwrap();
    let persisted = store.snapshot();

    let restarted_store = MemoryCounterStore::from_snapshot(persisted).unwrap();
    let restarted = DurableReplayGuard::new(restarted_store, ReplayPolicy::default());
    let (_, replayed, _) = authenticated();
    assert!(matches!(
        restarted.commit(replayed),
        Err(ReplayError::NotAdvanced | ReplayError::ChallengeAlreadyCommitted)
    ));
}

#[test]
fn durable_revocation_fences_old_generation_before_and_after_restart() {
    let (_, pending_old_response, vectors) = authenticated();
    let store = MemoryCounterStore::new();
    let guard = DurableReplayGuard::new(store.clone(), ReplayPolicy::default());
    assert_eq!(
        guard
            .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
            .unwrap(),
        RevokeOutcome::Revoked
    );
    assert_eq!(
        guard
            .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
            .unwrap(),
        RevokeOutcome::AlreadyRevoked
    );
    assert_eq!(
        guard.commit(pending_old_response).unwrap_err(),
        ReplayError::RevokedGeneration
    );

    let snapshot = store.snapshot();
    assert!(matches!(
        snapshot.entries(),
        [DurableReplayState::Revoked(value)]
            if value.mac_id() == vectors.mac_id()
                && value.device_id() == vectors.device_id()
                && value.pairing_generation() == vectors.generation()
    ));
    let restarted = DurableReplayGuard::new(
        MemoryCounterStore::from_snapshot(snapshot).unwrap(),
        ReplayPolicy::default(),
    );
    let (_, replayed_old_response, _) = authenticated();
    assert_eq!(
        restarted.commit(replayed_old_response).unwrap_err(),
        ReplayError::RevokedGeneration
    );
}

#[test]
fn only_a_strictly_newer_generation_can_initialize_from_a_tombstone() {
    let (_, generation_seven, vectors) = authenticated();
    let store = MemoryCounterStore::new();
    let guard = DurableReplayGuard::new(store.clone(), ReplayPolicy::default());
    let committed = guard.commit(generation_seven).unwrap();
    guard.finalize(committed).unwrap();

    let generation_eight = PairingGeneration::new(vectors.generation().get() + 1);
    let (_, response_eight_without_revoke, _) = authenticated_with_generation(generation_eight);
    assert_eq!(
        guard.commit(response_eight_without_revoke).unwrap_err(),
        ReplayError::GenerationRequiresRevocation
    );

    assert_eq!(
        guard
            .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
            .unwrap(),
        RevokeOutcome::Revoked
    );
    for generation in [
        PairingGeneration::new(vectors.generation().get() - 1),
        vectors.generation(),
    ] {
        let (_, stale, _) = authenticated_with_generation(generation);
        assert_eq!(
            guard.commit(stale).unwrap_err(),
            ReplayError::RevokedGeneration
        );
    }

    let (_, response_eight, _) = authenticated_with_generation(generation_eight);
    let committed = guard.commit(response_eight).unwrap();
    guard.finalize(committed).unwrap();
    assert!(matches!(
        store.snapshot().entries(),
        [DurableReplayState::Active(value)]
            if value.pairing_generation() == generation_eight && value.counter() == 42
    ));
}

#[test]
fn committed_token_can_only_be_finalized_by_the_originating_guard() {
    let (_, response, _) = authenticated();
    let store = MemoryCounterStore::new();
    let origin = DurableReplayGuard::new(store.clone(), ReplayPolicy::default());
    let other_guard_same_store = DurableReplayGuard::new(store, ReplayPolicy::default());
    let committed = origin.commit(response).unwrap();
    assert_eq!(
        other_guard_same_store.finalize(committed).unwrap_err(),
        ReplayError::WrongGuard
    );
}

#[test]
fn lying_store_cannot_turn_a_commit_acknowledgement_into_a_proof() {
    let (_, response, _) = authenticated();
    let guard = DurableReplayGuard::new(NeverPersistingStore, ReplayPolicy::default());
    let committed = guard.commit(response).unwrap();
    assert_eq!(
        guard.finalize(committed).unwrap_err(),
        ReplayError::ReceiptMismatch
    );

    let (_, response, vectors) = authenticated();
    let store = WrongGenerationStore::new(PairingGeneration::new(vectors.generation().get() + 1));
    let guard = DurableReplayGuard::new(store, ReplayPolicy::default());
    let committed = guard.commit(response).unwrap();
    assert_eq!(
        guard.finalize(committed).unwrap_err(),
        ReplayError::ReceiptMismatch
    );
}

#[test]
fn revoke_store_failure_or_contention_yields_no_successful_revocation() {
    let vectors = common::Vectors::load();
    let failing = DurableReplayGuard::new(FailingStore, ReplayPolicy::default());
    assert_eq!(
        failing
            .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
            .unwrap_err(),
        ReplayError::Store(ReplayStoreError::Unavailable)
    );
    let contended = DurableReplayGuard::new(AlwaysContendedStore, ReplayPolicy::default());
    assert_eq!(
        contended
            .revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
            .unwrap_err(),
        ReplayError::ConcurrentUpdate
    );
}

#[test]
fn concurrent_revoke_and_pending_commit_never_finalize_an_old_proof_after_revocation() {
    let (_, response, vectors) = authenticated();
    let guard = Arc::new(DurableReplayGuard::new(
        MemoryCounterStore::new(),
        ReplayPolicy::default(),
    ));
    let barrier = Arc::new(Barrier::new(2));
    let commit_guard = Arc::clone(&guard);
    let commit_barrier = Arc::clone(&barrier);
    let commit_worker = thread::spawn(move || {
        commit_barrier.wait();
        commit_guard.commit(response)
    });
    let revoke_guard = Arc::clone(&guard);
    let revoke_barrier = Arc::clone(&barrier);
    let revoke_worker = thread::spawn(move || {
        revoke_barrier.wait();
        revoke_guard.revoke(vectors.mac_id(), vectors.device_id(), vectors.generation())
    });

    let committed = commit_worker.join().unwrap().ok();
    let revoked = revoke_worker.join().unwrap().is_ok();
    let proof = committed.and_then(|token| guard.finalize(token).ok());
    assert!(!(revoked && proof.is_some()));
    assert!(revoked || proof.is_some());
}

#[test]
fn memory_cas_expected_old_state_binds_the_complete_session() {
    use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch};

    let vectors = common::Vectors::load();
    let active = DurableCounterState::new(
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
        vectors.binding(),
        1,
        42,
    )
    .unwrap();
    let wrong_binding = SessionBinding::new(
        LockEpoch::new(vectors.binding().lock_epoch().get() + 1),
        AuditSessionId::new(vectors.binding().audit_session_id().get()),
        ConsoleUid::new(vectors.binding().console_uid().get()),
    );
    let wrong_expected = DurableReplayState::Active(
        DurableCounterState::new(
            vectors.mac_id(),
            vectors.device_id(),
            vectors.generation(),
            wrong_binding,
            1,
            42,
        )
        .unwrap(),
    );
    let replacement = DurableReplayState::Active(
        DurableCounterState::new(
            vectors.mac_id(),
            vectors.device_id(),
            vectors.generation(),
            vectors.binding(),
            2,
            43,
        )
        .unwrap(),
    );
    let store = MemoryCounterStore::from_entries([active]).unwrap();
    assert!(
        !store
            .compare_and_swap(vectors.device_id(), Some(&wrong_expected), &replacement)
            .unwrap()
    );
}

struct NeverPersistingStore;

impl CounterStore for NeverPersistingStore {
    fn load(&self, _device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        Ok(None)
    }

    fn compare_and_swap(
        &self,
        _device_id: DeviceId,
        _expected: Option<&DurableReplayState>,
        _replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        Ok(true)
    }
}

struct WrongGenerationStore {
    generation: PairingGeneration,
    value: std::sync::Mutex<Option<DurableReplayState>>,
}

impl WrongGenerationStore {
    fn new(generation: PairingGeneration) -> Self {
        Self {
            generation,
            value: std::sync::Mutex::new(None),
        }
    }
}

impl CounterStore for WrongGenerationStore {
    fn load(&self, _device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        Ok(*self.value.lock().unwrap())
    }

    fn compare_and_swap(
        &self,
        _device_id: DeviceId,
        _expected: Option<&DurableReplayState>,
        replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        let DurableReplayState::Active(active) = replacement else {
            return Err(ReplayStoreError::Corrupt);
        };
        let wrong = DurableCounterState::new(
            active.mac_id(),
            active.device_id(),
            self.generation,
            active.binding(),
            active.challenge_id(),
            active.counter(),
        )
        .unwrap();
        *self.value.lock().unwrap() = Some(DurableReplayState::Active(wrong));
        Ok(true)
    }
}

struct FailingStore;

impl CounterStore for FailingStore {
    fn load(&self, _device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        Err(ReplayStoreError::Unavailable)
    }

    fn compare_and_swap(
        &self,
        _device_id: DeviceId,
        _expected: Option<&DurableReplayState>,
        _replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        panic!("CAS must not run after a load failure")
    }
}

struct AlwaysContendedStore;

impl CounterStore for AlwaysContendedStore {
    fn load(&self, _device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        Ok(None)
    }

    fn compare_and_swap(
        &self,
        _device_id: DeviceId,
        _expected: Option<&DurableReplayState>,
        _replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        Ok(false)
    }
}

struct ReturnedStateStore(DurableCounterState);

impl CounterStore for ReturnedStateStore {
    fn load(&self, _device_id: DeviceId) -> Result<Option<DurableReplayState>, ReplayStoreError> {
        Ok(Some(self.0.into()))
    }

    fn compare_and_swap(
        &self,
        _device_id: DeviceId,
        _expected: Option<&DurableReplayState>,
        _replacement: &DurableReplayState,
    ) -> Result<bool, ReplayStoreError> {
        panic!("CAS must not run for a corrupt snapshot")
    }
}

fn _assert_state_fields_are_bound(
    _mac: MacId,
    _device: DeviceId,
    _generation: PairingGeneration,
    _binding: SessionBinding,
) {
}
