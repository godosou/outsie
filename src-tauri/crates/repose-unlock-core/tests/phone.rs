mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey};
use repose_unlock_core::domain::{LockEpoch, MonoMillis};
use repose_unlock_core::phone::{
    DurablePhoneState, MemoryPhoneResponseStore, PairingRotateOutcome, PhoneResponseCoordinator,
    PhoneResponseError, PhoneResponseStore, PhoneStateError, PhoneStoreError,
};
use repose_unlock_core::protocol::crypto::{
    CryptoRandom, IdentitySigningError, IssueParameters, IssuedChallenge, PairedMac,
    PhoneBuildError, PhoneResponseSigner, PhoneResponseSigningRequest, RandomError,
    VerifiedMacChallenge, issue_challenge, verify_mac_challenge,
};
use repose_unlock_core::protocol::messages::{DeviceId, MacId, PairingGeneration, PublicKeyBytes};
use repose_unlock_core::protocol::wire::{RESPONSE_FRAME_LEN, decode_response};
use repose_unlock_core::state_machine::{
    Effect, Event, SessionBinding, TimingPolicy, UnlockState, transition,
};
use sha2::{Digest, Sha256};

use common::{TestMacSigner, Vectors, ms};

fn response_entropy(vectors: &Vectors) -> Vec<u8> {
    let mut bytes = vectors.bytes("phone_ephemeral_private_key_test_only");
    bytes.extend(vectors.bytes("phone_nonce"));
    bytes
}

struct CountingRandom {
    bytes: Vec<u8>,
    offset: usize,
    calls: Arc<AtomicUsize>,
}

impl CountingRandom {
    fn new(bytes: Vec<u8>, calls: Arc<AtomicUsize>) -> Self {
        Self {
            bytes,
            offset: 0,
            calls,
        }
    }
}

impl CryptoRandom for CountingRandom {
    fn fill_bytes(&mut self, output: &mut [u8]) -> Result<(), RandomError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let end = self.offset.checked_add(output.len()).ok_or(RandomError)?;
        let source = self.bytes.get(self.offset..end).ok_or(RandomError)?;
        output.copy_from_slice(source);
        self.offset = end;
        Ok(())
    }
}

struct CountingPhoneSigner {
    key: SigningKey,
    calls: Arc<AtomicUsize>,
}

impl CountingPhoneSigner {
    fn new(vectors: &Vectors, calls: Arc<AtomicUsize>) -> Self {
        Self {
            key: SigningKey::from_slice(&vectors.bytes("phone_signing_private_key_test_only"))
                .unwrap(),
            calls,
        }
    }
}

impl PhoneResponseSigner for CountingPhoneSigner {
    fn sign_response(
        &mut self,
        request: &PhoneResponseSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let signature: Signature = self
            .key
            .sign_prehash(request.prehash())
            .map_err(|_| IdentitySigningError)?;
        Ok(signature
            .normalize_s()
            .unwrap_or(signature)
            .to_bytes()
            .into())
    }
}

fn binding_with_epoch(vectors: &Vectors, epoch: u64) -> SessionBinding {
    SessionBinding::new(
        LockEpoch::new(epoch),
        vectors.binding().audit_session_id(),
        vectors.binding().console_uid(),
    )
}

fn signed_challenge(
    vectors: &Vectors,
    binding: SessionBinding,
    challenge_number: u64,
    generation: PairingGeneration,
    counter_floor: u64,
    nonce_tweak: u8,
) -> IssuedChallenge {
    signed_challenge_for_mac(
        vectors,
        vectors.mac_id(),
        binding,
        challenge_number,
        generation,
        counter_floor,
        nonce_tweak,
    )
}

fn signed_challenge_for_mac(
    vectors: &Vectors,
    mac_id: MacId,
    binding: SessionBinding,
    challenge_number: u64,
    generation: PairingGeneration,
    counter_floor: u64,
    nonce_tweak: u8,
) -> IssuedChallenge {
    let policy = TimingPolicy::new(vectors.u64("ttl_ms"), 3_000, 1).unwrap();
    let (mut state, _) = transition(
        UnlockState::unlocked(policy),
        Event::SessionLocked { binding },
        ms(10),
    )
    .unwrap();
    (state, _) = transition(state, Event::FarStable { binding }, ms(11)).unwrap();

    let mut now = 12;
    loop {
        let (challenging, effects) =
            transition(state, Event::NearStable { binding }, ms(now)).unwrap();
        let [effect]: [Effect; 1] = effects.try_into().unwrap();
        let Effect::StartChallenge(request) = effect else {
            panic!("expected challenge request")
        };
        if request.challenge_id().get() == challenge_number {
            let mut nonce = vectors.bytes("mac_nonce");
            nonce[0] ^= nonce_tweak;
            let mut entropy = vectors.bytes("mac_ephemeral_private_key_test_only");
            entropy.extend(nonce);
            let rng_calls = Arc::new(AtomicUsize::new(0));
            let mut rng = CountingRandom::new(entropy, rng_calls);
            let mut signer = TestMacSigner::from_vectors(vectors);
            return issue_challenge(
                request,
                IssueParameters::new(
                    mac_id,
                    vectors.device_id(),
                    generation,
                    counter_floor,
                    MonoMillis::new(now),
                    PublicKeyBytes::try_new(vectors.array("mac_signing_public_key")).unwrap(),
                ),
                &mut rng,
                &mut signer,
            )
            .unwrap();
        }
        let challenge_id = request.challenge_id();
        (state, _) = transition(
            challenging,
            Event::ChallengeFailed {
                binding,
                challenge_id,
            },
            ms(now + 1),
        )
        .unwrap();
        now += 2;
    }
}

#[test]
fn durable_cache_key_includes_both_mac_and_device_identity() {
    let vectors = Vectors::load();
    let other_mac = MacId::new([0xa5; 16]);
    let first = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    let second = signed_challenge_for_mac(
        &vectors,
        other_mac,
        vectors.binding(),
        1,
        vectors.generation(),
        0,
        7,
    );
    let store = MemoryPhoneResponseStore::new();
    let coordinator = PhoneResponseCoordinator::new(store.clone());
    initialize_pairing(
        &coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    initialize_pairing(
        &coordinator,
        other_mac,
        vectors.device_id(),
        vectors.generation(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    coordinator
        .respond(
            verified(&vectors, &first, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();

    let other_pairing = PairedMac::new(
        other_mac,
        vectors.device_id(),
        vectors.generation(),
        PublicKeyBytes::try_new(vectors.array("mac_signing_public_key")).unwrap(),
    );
    let other_verified = verify_mac_challenge(second.frame(), &other_pairing).unwrap();
    let mut rng = CountingRandom::new(response_entropy(&vectors), calls);
    coordinator
        .respond(other_verified, &mut rng, &mut signer)
        .unwrap();
    assert_eq!(store.snapshot().entries().len(), 2);
}

fn verified(
    vectors: &Vectors,
    issued: &IssuedChallenge,
    generation: PairingGeneration,
) -> VerifiedMacChallenge {
    let paired = PairedMac::new(
        vectors.mac_id(),
        vectors.device_id(),
        generation,
        PublicKeyBytes::try_new(vectors.array("mac_signing_public_key")).unwrap(),
    );
    verify_mac_challenge(issued.frame(), &paired).unwrap()
}

fn initialize_pairing<S: PhoneResponseStore>(
    coordinator: &PhoneResponseCoordinator<S>,
    mac_id: MacId,
    device_id: DeviceId,
    generation: PairingGeneration,
) {
    assert_eq!(
        coordinator
            .rotate_pairing(mac_id, device_id, generation)
            .unwrap(),
        PairingRotateOutcome::Initialized
    );
}

#[test]
fn missing_durable_pairing_state_never_silently_resets_the_counter() {
    let vectors = Vectors::load();
    let issued = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    let store = MemoryPhoneResponseStore::new();
    let coordinator = PhoneResponseCoordinator::new(store.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));

    assert_eq!(
        coordinator
            .respond(
                verified(&vectors, &issued, vectors.generation()),
                &mut rng,
                &mut signer,
            )
            .unwrap_err(),
        PhoneResponseError::UninitializedPairing
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(store.snapshot().entries().is_empty());
}

#[test]
fn exact_retransmission_returns_committed_bytes_without_crypto_or_counter_advance() {
    let vectors = Vectors::load();
    let issued = signed_challenge(
        &vectors,
        vectors.binding(),
        1,
        vectors.generation(),
        vectors.u64("counter_floor"),
        0,
    );
    let store = MemoryPhoneResponseStore::new();
    let coordinator = PhoneResponseCoordinator::new(store.clone());
    initialize_pairing(
        &coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let rng_calls = Arc::new(AtomicUsize::new(0));
    let signer_calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&rng_calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&signer_calls));
    let first = coordinator
        .respond(
            verified(&vectors, &issued, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();
    assert_eq!(first.as_slice(), vectors.bytes("response_frame"));
    assert_eq!(rng_calls.load(Ordering::SeqCst), 2);
    assert_eq!(signer_calls.load(Ordering::SeqCst), 1);

    let mut fail_rng = CountingRandom::new(Vec::new(), Arc::clone(&rng_calls));
    let mut fail_signer = CountingPhoneSigner::new(&vectors, Arc::clone(&signer_calls));
    let retransmission = coordinator
        .respond(
            verified(&vectors, &issued, vectors.generation()),
            &mut fail_rng,
            &mut fail_signer,
        )
        .unwrap();
    assert_eq!(retransmission, first);
    assert_eq!(rng_calls.load(Ordering::SeqCst), 2);
    assert_eq!(signer_calls.load(Ordering::SeqCst), 1);

    let fingerprint: [u8; 32] = Sha256::digest(issued.frame()).into();
    let snapshot = store.snapshot();
    let [entry] = snapshot.entries() else {
        panic!("one durable phone state")
    };
    assert_eq!(entry.challenge_fingerprint(), Some(&fingerprint));
    assert_eq!(entry.counter(), 42);
    assert_eq!(entry.response(), Some(&first));
}

#[test]
fn one_hundred_concurrent_replays_generate_once_and_return_identical_bytes() {
    let vectors = Vectors::load();
    let issued = signed_challenge(
        &vectors,
        vectors.binding(),
        1,
        vectors.generation(),
        vectors.u64("counter_floor"),
        0,
    );
    let coordinator = Arc::new(PhoneResponseCoordinator::new(
        MemoryPhoneResponseStore::new(),
    ));
    initialize_pairing(
        coordinator.as_ref(),
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let rng_calls = Arc::new(AtomicUsize::new(0));
    let signer_calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(100));
    let workers: Vec<_> = (0..100)
        .map(|_| {
            let challenge = verified(&vectors, &issued, vectors.generation());
            let coordinator = Arc::clone(&coordinator);
            let barrier = Arc::clone(&barrier);
            let rng_calls = Arc::clone(&rng_calls);
            let signer_calls = Arc::clone(&signer_calls);
            let entropy = response_entropy(&vectors);
            let signer = CountingPhoneSigner::new(&vectors, signer_calls);
            thread::spawn(move || {
                let mut rng = CountingRandom::new(entropy, rng_calls);
                let mut signer = signer;
                barrier.wait();
                coordinator
                    .respond(challenge, &mut rng, &mut signer)
                    .unwrap()
            })
        })
        .collect();
    let responses: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert!(responses.iter().all(|response| response == &responses[0]));
    assert_eq!(rng_calls.load(Ordering::SeqCst), 2);
    assert_eq!(signer_calls.load(Ordering::SeqCst), 1);
    assert_eq!(decode_response(&responses[0]).unwrap().counter(), 42);
}

#[test]
fn restart_recovers_cached_response_and_newer_challenge_advances_once() {
    let vectors = Vectors::load();
    let first = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    let store = MemoryPhoneResponseStore::new();
    let coordinator = PhoneResponseCoordinator::new(store.clone());
    initialize_pairing(
        &coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    let first_response = coordinator
        .respond(
            verified(&vectors, &first, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();

    let restarted = PhoneResponseCoordinator::new(
        MemoryPhoneResponseStore::from_snapshot(store.snapshot()).unwrap(),
    );
    let calls_before = calls.load(Ordering::SeqCst);
    let mut no_rng = CountingRandom::new(Vec::new(), Arc::clone(&calls));
    let mut no_signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    assert_eq!(
        restarted
            .respond(
                verified(&vectors, &first, vectors.generation()),
                &mut no_rng,
                &mut no_signer,
            )
            .unwrap(),
        first_response
    );
    assert_eq!(calls.load(Ordering::SeqCst), calls_before);

    let second = signed_challenge(&vectors, vectors.binding(), 2, vectors.generation(), 0, 1);
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    let response = restarted
        .respond(
            verified(&vectors, &second, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();
    assert_eq!(decode_response(&response).unwrap().counter(), 43);
}

#[test]
fn changed_same_challenge_and_stale_challenges_fail_closed() {
    let vectors = Vectors::load();
    let store = MemoryPhoneResponseStore::new();
    let coordinator = PhoneResponseCoordinator::new(store);
    initialize_pairing(
        &coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let first = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    coordinator
        .respond(
            verified(&vectors, &first, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();

    let conflict = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 9);
    let calls_before = calls.load(Ordering::SeqCst);
    assert_eq!(
        coordinator
            .respond(
                verified(&vectors, &conflict, vectors.generation()),
                &mut rng,
                &mut signer,
            )
            .unwrap_err(),
        PhoneResponseError::ChallengeConflict
    );
    assert_eq!(calls.load(Ordering::SeqCst), calls_before);

    let newer = signed_challenge(&vectors, vectors.binding(), 2, vectors.generation(), 41, 1);
    let mut rng2 = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    coordinator
        .respond(
            verified(&vectors, &newer, vectors.generation()),
            &mut rng2,
            &mut signer,
        )
        .unwrap();
    assert_eq!(
        coordinator
            .respond(
                verified(&vectors, &first, vectors.generation()),
                &mut rng,
                &mut signer,
            )
            .unwrap_err(),
        PhoneResponseError::StaleChallenge
    );

    let lower_epoch = signed_challenge(
        &vectors,
        binding_with_epoch(&vectors, vectors.binding().lock_epoch().get() - 1),
        3,
        vectors.generation(),
        41,
        2,
    );
    assert_eq!(
        coordinator
            .respond(
                verified(&vectors, &lower_epoch, vectors.generation()),
                &mut rng,
                &mut signer,
            )
            .unwrap_err(),
        PhoneResponseError::StaleChallenge
    );
}

#[test]
fn higher_epoch_allows_restarted_challenge_id_but_same_epoch_reuse_does_not() {
    let vectors = Vectors::load();
    let coordinator = PhoneResponseCoordinator::new(MemoryPhoneResponseStore::new());
    initialize_pairing(
        &coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let first = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    coordinator
        .respond(
            verified(&vectors, &first, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();

    let restarted = signed_challenge(
        &vectors,
        binding_with_epoch(&vectors, vectors.binding().lock_epoch().get() + 1),
        1,
        vectors.generation(),
        0,
        3,
    );
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let response = coordinator
        .respond(
            verified(&vectors, &restarted, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();
    assert_eq!(decode_response(&response).unwrap().counter(), 43);
}

#[test]
fn pairing_generation_change_requires_explicit_rotation() {
    let vectors = Vectors::load();
    let store = MemoryPhoneResponseStore::new();
    let coordinator = PhoneResponseCoordinator::new(store);
    initialize_pairing(
        &coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let first = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    coordinator
        .respond(
            verified(&vectors, &first, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();

    let next_generation = PairingGeneration::new(vectors.generation().get() + 1);
    let next = signed_challenge(&vectors, vectors.binding(), 1, next_generation, 0, 4);
    assert_eq!(
        coordinator
            .respond(
                verified(&vectors, &next, next_generation),
                &mut rng,
                &mut signer,
            )
            .unwrap_err(),
        PhoneResponseError::PairingRotationRequired
    );
    assert_eq!(
        coordinator
            .rotate_pairing(vectors.mac_id(), vectors.device_id(), next_generation)
            .unwrap(),
        PairingRotateOutcome::Rotated
    );
    let mut rng = CountingRandom::new(response_entropy(&vectors), calls);
    let response = coordinator
        .respond(
            verified(&vectors, &next, next_generation),
            &mut rng,
            &mut signer,
        )
        .unwrap();
    assert_eq!(decode_response(&response).unwrap().counter(), 1);
}

#[test]
fn counter_overflow_fails_before_randomness_or_signing() {
    let vectors = Vectors::load();
    let issued = signed_challenge(
        &vectors,
        vectors.binding(),
        1,
        vectors.generation(),
        u64::MAX,
        0,
    );
    let coordinator = PhoneResponseCoordinator::new(MemoryPhoneResponseStore::new());
    initialize_pairing(
        &coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(Vec::new(), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    assert_eq!(
        coordinator
            .respond(
                verified(&vectors, &issued, vectors.generation()),
                &mut rng,
                &mut signer,
            )
            .unwrap_err(),
        PhoneResponseError::CounterOverflow
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct FailingStore {
    fail_load: bool,
    ready: DurablePhoneState,
}

impl PhoneResponseStore for FailingStore {
    fn load(
        &self,
        _mac_id: MacId,
        _device_id: DeviceId,
    ) -> Result<Option<DurablePhoneState>, PhoneStoreError> {
        if self.fail_load {
            Err(PhoneStoreError::Unavailable)
        } else {
            Ok(Some(self.ready.clone()))
        }
    }

    fn compare_and_swap(
        &self,
        _mac_id: MacId,
        _device_id: DeviceId,
        _expected: Option<&DurablePhoneState>,
        _replacement: &DurablePhoneState,
    ) -> Result<bool, PhoneStoreError> {
        Err(PhoneStoreError::Unavailable)
    }
}

#[test]
fn store_failures_never_return_an_uncommitted_response() {
    let vectors = Vectors::load();
    let issued = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    for fail_load in [true, false] {
        let coordinator = PhoneResponseCoordinator::new(FailingStore {
            fail_load,
            ready: DurablePhoneState::ready(
                vectors.mac_id(),
                vectors.device_id(),
                vectors.generation(),
            ),
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
        let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
        assert_eq!(
            coordinator
                .respond(
                    verified(&vectors, &issued, vectors.generation()),
                    &mut rng,
                    &mut signer,
                )
                .unwrap_err(),
            PhoneResponseError::Store(PhoneStoreError::Unavailable)
        );
    }
}

struct FailingSigner;

impl PhoneResponseSigner for FailingSigner {
    fn sign_response(
        &mut self,
        _request: &PhoneResponseSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError> {
        Err(IdentitySigningError)
    }
}

#[test]
fn randomness_and_signer_errors_leave_no_durable_response_or_counter() {
    let vectors = Vectors::load();
    let issued = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);

    let random_store = MemoryPhoneResponseStore::new();
    let random_coordinator = PhoneResponseCoordinator::new(random_store.clone());
    initialize_pairing(
        &random_coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut no_rng = CountingRandom::new(Vec::new(), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, Arc::clone(&calls));
    assert_eq!(
        random_coordinator
            .respond(
                verified(&vectors, &issued, vectors.generation()),
                &mut no_rng,
                &mut signer,
            )
            .unwrap_err(),
        PhoneResponseError::Build(PhoneBuildError::Randomness)
    );
    let random_snapshot = random_store.snapshot();
    let [random_state] = random_snapshot.entries() else {
        panic!("ready state remains")
    };
    assert_eq!(random_state.counter(), 0);
    assert!(random_state.response().is_none());

    let signer_store = MemoryPhoneResponseStore::new();
    let signer_coordinator = PhoneResponseCoordinator::new(signer_store.clone());
    initialize_pairing(
        &signer_coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let mut rng = CountingRandom::new(response_entropy(&vectors), calls);
    assert_eq!(
        signer_coordinator
            .respond(
                verified(&vectors, &issued, vectors.generation()),
                &mut rng,
                &mut FailingSigner,
            )
            .unwrap_err(),
        PhoneResponseError::Build(PhoneBuildError::Signing)
    );
    let signer_snapshot = signer_store.snapshot();
    let [signer_state] = signer_snapshot.entries() else {
        panic!("ready state remains")
    };
    assert_eq!(signer_state.counter(), 0);
    assert!(signer_state.response().is_none());
}

struct AcknowledgingWithoutPersistence {
    ready: DurablePhoneState,
}

impl PhoneResponseStore for AcknowledgingWithoutPersistence {
    fn load(
        &self,
        _mac_id: MacId,
        _device_id: DeviceId,
    ) -> Result<Option<DurablePhoneState>, PhoneStoreError> {
        Ok(Some(self.ready.clone()))
    }

    fn compare_and_swap(
        &self,
        _mac_id: MacId,
        _device_id: DeviceId,
        _expected: Option<&DurablePhoneState>,
        _replacement: &DurablePhoneState,
    ) -> Result<bool, PhoneStoreError> {
        Ok(true)
    }
}

#[test]
fn false_durable_acknowledgement_never_releases_candidate_response() {
    let vectors = Vectors::load();
    let issued = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    let coordinator = PhoneResponseCoordinator::new(AcknowledgingWithoutPersistence {
        ready: DurablePhoneState::ready(
            vectors.mac_id(),
            vectors.device_id(),
            vectors.generation(),
        ),
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, calls);
    assert_eq!(
        coordinator
            .respond(
                verified(&vectors, &issued, vectors.generation()),
                &mut rng,
                &mut signer,
            )
            .unwrap_err(),
        PhoneResponseError::CommitMismatch
    );
}

#[test]
fn durable_adapter_can_reconstruct_only_a_self_consistent_cached_record() {
    let vectors = Vectors::load();
    let issued = signed_challenge(&vectors, vectors.binding(), 1, vectors.generation(), 41, 0);
    let store = MemoryPhoneResponseStore::new();
    let coordinator = PhoneResponseCoordinator::new(store.clone());
    initialize_pairing(
        &coordinator,
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut rng = CountingRandom::new(response_entropy(&vectors), Arc::clone(&calls));
    let mut signer = CountingPhoneSigner::new(&vectors, calls);
    coordinator
        .respond(
            verified(&vectors, &issued, vectors.generation()),
            &mut rng,
            &mut signer,
        )
        .unwrap();
    let snapshot = store.snapshot();
    let [entry] = snapshot.entries() else {
        panic!("cached entry")
    };
    let reconstructed = DurablePhoneState::try_cached(
        entry.mac_id(),
        entry.device_id(),
        entry.pairing_generation(),
        entry.binding().unwrap(),
        entry.challenge_id().unwrap().get(),
        *entry.challenge_fingerprint().unwrap(),
        entry.counter(),
        *entry.response().unwrap(),
    )
    .unwrap();
    assert_eq!(&reconstructed, entry);

    let mut wrong_response = *entry.response().unwrap();
    wrong_response[76] ^= 1;
    assert_eq!(
        DurablePhoneState::try_cached(
            entry.mac_id(),
            entry.device_id(),
            entry.pairing_generation(),
            entry.binding().unwrap(),
            entry.challenge_id().unwrap().get(),
            *entry.challenge_fingerprint().unwrap(),
            entry.counter(),
            wrong_response,
        )
        .unwrap_err(),
        PhoneStateError::ResponseMismatch
    );
}

const _: fn([u8; RESPONSE_FRAME_LEN]) = |_| {};
const _: fn(MacId, DeviceId) = |_, _| {};
