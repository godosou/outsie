mod common;

use std::sync::{Arc, Barrier};
use std::thread;

use repose_unlock_core::protocol::messages::{DeviceId, MacId, PairingGeneration};
use repose_unlock_core::replay::{
    CounterStore, DurableCounterState, DurableReplayGuard, MemoryCounterStore, ReplayError,
    ReplayPolicy, ReplayStoreError,
};
use repose_unlock_core::state_machine::SessionBinding;

use common::{authenticated, authenticated_with_counter};

#[test]
fn absent_state_means_zero_and_a_valid_large_non_unit_increment_commits() {
    let (_, response, _) = authenticated();
    let store = MemoryCounterStore::new();
    let guard = DurableReplayGuard::new(store.clone(), ReplayPolicy::default());
    let committed = guard.commit(response).unwrap();
    assert_eq!(committed.counter(), 42);
    assert_eq!(store.snapshot().entries().len(), 1);
    assert_eq!(store.snapshot().entries()[0].counter(), 42);
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
                guard.commit(response).is_ok()
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
                guard.commit(response).is_ok()
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
        ReplayError::CorruptSnapshot
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

struct FailingStore;

impl CounterStore for FailingStore {
    fn load(&self, _device_id: DeviceId) -> Result<Option<DurableCounterState>, ReplayStoreError> {
        Err(ReplayStoreError::Unavailable)
    }

    fn compare_and_swap(
        &self,
        _device_id: DeviceId,
        _expected: Option<&DurableCounterState>,
        _replacement: &DurableCounterState,
    ) -> Result<bool, ReplayStoreError> {
        panic!("CAS must not run after a load failure")
    }
}

struct AlwaysContendedStore;

impl CounterStore for AlwaysContendedStore {
    fn load(&self, _device_id: DeviceId) -> Result<Option<DurableCounterState>, ReplayStoreError> {
        Ok(None)
    }

    fn compare_and_swap(
        &self,
        _device_id: DeviceId,
        _expected: Option<&DurableCounterState>,
        _replacement: &DurableCounterState,
    ) -> Result<bool, ReplayStoreError> {
        Ok(false)
    }
}

struct ReturnedStateStore(DurableCounterState);

impl CounterStore for ReturnedStateStore {
    fn load(&self, _device_id: DeviceId) -> Result<Option<DurableCounterState>, ReplayStoreError> {
        Ok(Some(self.0))
    }

    fn compare_and_swap(
        &self,
        _device_id: DeviceId,
        _expected: Option<&DurableCounterState>,
        _replacement: &DurableCounterState,
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
