#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey};
use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};
use repose_unlock_core::protocol::crypto::{
    AuthenticatedResponse, CryptoRandom, IdentitySigningError, IssueParameters, IssuedChallenge,
    MacChallengeSigner, MacChallengeSigningRequest, PairedDevice, PairedMac, PhoneResponseSigner,
    PhoneResponseSigningRequest, RandomError, VerificationContext, build_phone_response,
    issue_challenge, verify_mac_challenge, verify_response,
};
use repose_unlock_core::protocol::messages::{DeviceId, MacId, PairingGeneration, PublicKeyBytes};
use repose_unlock_core::protocol::wire::{RESPONSE_FRAME_LEN, RESPONSE_SIGNATURE_OFFSET};
use repose_unlock_core::state_machine::{
    ChallengeRequest, Effect, Event, Permit, SessionBinding, TimingPolicy, UnlockState, transition,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub struct TestMacSigner(SigningKey);

impl TestMacSigner {
    pub fn from_vectors(vectors: &Vectors) -> Self {
        Self(SigningKey::from_slice(&vectors.bytes("mac_signing_private_key_test_only")).unwrap())
    }
}

impl MacChallengeSigner for TestMacSigner {
    fn sign_challenge(
        &mut self,
        request: &MacChallengeSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError> {
        let signature: Signature = self
            .0
            .sign_prehash(request.prehash())
            .map_err(|_| IdentitySigningError)?;
        Ok(signature
            .normalize_s()
            .unwrap_or(signature)
            .to_bytes()
            .into())
    }
}

pub struct TestPhoneSigner(SigningKey);

impl TestPhoneSigner {
    pub fn from_vectors(vectors: &Vectors) -> Self {
        Self(SigningKey::from_slice(&vectors.bytes("phone_signing_private_key_test_only")).unwrap())
    }
}

impl PhoneResponseSigner for TestPhoneSigner {
    fn sign_response(
        &mut self,
        request: &PhoneResponseSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError> {
        let signature: Signature = self
            .0
            .sign_prehash(request.prehash())
            .map_err(|_| IdentitySigningError)?;
        Ok(signature
            .normalize_s()
            .unwrap_or(signature)
            .to_bytes()
            .into())
    }
}

pub struct Vectors(Value);

impl Vectors {
    pub fn load() -> Self {
        let path = fixture_path("crypto-vectors.json");
        let source = fs::read_to_string(path).expect("read committed vectors");
        Self(serde_json::from_str(&source).expect("parse committed vectors"))
    }

    pub fn bytes(&self, name: &str) -> Vec<u8> {
        decode_hex(self.0[name].as_str().expect("hex vector string"))
    }

    pub fn array<const N: usize>(&self, name: &str) -> [u8; N] {
        self.bytes(name).try_into().expect("fixed vector length")
    }

    pub fn u64(&self, name: &str) -> u64 {
        match &self.0[name] {
            Value::String(value) => value.parse().expect("u64 string"),
            Value::Number(value) => value.as_u64().expect("u64 number"),
            _ => panic!("expected u64 vector {name}"),
        }
    }

    pub fn mac_id(&self) -> MacId {
        MacId::new(self.array("mac_id"))
    }

    pub fn device_id(&self) -> DeviceId {
        DeviceId::new(self.array("device_id"))
    }

    pub fn generation(&self) -> PairingGeneration {
        PairingGeneration::new(self.u64("pairing_generation"))
    }

    pub fn binding(&self) -> SessionBinding {
        SessionBinding::new(
            LockEpoch::new(self.u64("lock_epoch")),
            AuditSessionId::new(self.u64("audit_session_id") as u32),
            ConsoleUid::new(self.u64("console_uid") as u32),
        )
    }

    pub fn pairing(&self) -> PairedDevice {
        PairedDevice::new(
            self.mac_id(),
            self.device_id(),
            self.generation(),
            PublicKeyBytes::try_new(self.array("phone_signing_public_key"))
                .expect("valid fixture identity key"),
        )
    }

    pub fn paired_mac(&self) -> PairedMac {
        PairedMac::new(
            self.mac_id(),
            self.device_id(),
            self.generation(),
            PublicKeyBytes::try_new(self.array("mac_signing_public_key"))
                .expect("valid fixture Mac identity key"),
        )
    }
}

pub struct FixedRandom {
    bytes: Vec<u8>,
    offset: usize,
}

impl FixedRandom {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, offset: 0 }
    }
}

impl CryptoRandom for FixedRandom {
    fn fill_bytes(&mut self, output: &mut [u8]) -> Result<(), RandomError> {
        let end = self.offset.checked_add(output.len()).ok_or(RandomError)?;
        let source = self.bytes.get(self.offset..end).ok_or(RandomError)?;
        output.copy_from_slice(source);
        self.offset = end;
        Ok(())
    }
}

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../protocol/fixtures/v1")
        .join(name)
}

pub fn fixture_challenge() -> Vec<u8> {
    fs::read(fixture_path("challenge.bin")).expect("read binary challenge fixture")
}

pub fn ms(value: u64) -> MonoMillis {
    MonoMillis::new(value)
}

pub fn issued() -> (UnlockState, IssuedChallenge, Vectors) {
    let vectors = Vectors::load();
    let (challenging, request) = challenge_request(&vectors);

    let mut entropy = vectors.bytes("mac_ephemeral_private_key_test_only");
    entropy.extend(vectors.bytes("mac_nonce"));
    let mut rng = FixedRandom::new(entropy);
    let mut signer = TestMacSigner::from_vectors(&vectors);
    let issued = issue_challenge(
        request,
        IssueParameters::new(
            vectors.mac_id(),
            vectors.device_id(),
            vectors.generation(),
            vectors.u64("counter_floor"),
            ms(vectors.u64("issued_at_ms")),
            PublicKeyBytes::try_new(vectors.array("mac_signing_public_key")).unwrap(),
        ),
        &mut rng,
        &mut signer,
    )
    .expect("issue fixture challenge");
    (challenging, issued, vectors)
}

pub fn challenge_request(vectors: &Vectors) -> (UnlockState, ChallengeRequest) {
    let binding = vectors.binding();
    let policy = TimingPolicy::new(vectors.u64("ttl_ms"), 3_000, 1_000).unwrap();
    let (locked, _) = transition(
        UnlockState::unlocked(policy),
        Event::SessionLocked { binding },
        ms(998),
    )
    .unwrap();
    let (armed, _) = transition(locked, Event::FarStable { binding }, ms(999)).unwrap();
    let (challenging, effects) =
        transition(armed, Event::NearStable { binding }, ms(1_000)).unwrap();
    let [effect] = effects.try_into().expect("one challenge effect");
    let Effect::StartChallenge(request) = effect else {
        panic!("expected start challenge effect");
    };
    (challenging, request)
}

pub fn authenticated() -> (UnlockState, AuthenticatedResponse, Vectors) {
    authenticated_with_counter(42)
}

pub fn authenticated_with_counter(counter: u64) -> (UnlockState, AuthenticatedResponse, Vectors) {
    let (state, issued, vectors) = issued();
    let response = response_for_counter(&issued, &vectors, counter, counter as u8);
    let context = VerificationContext::new(vectors.binding(), vectors.pairing());
    let authenticated = verify_response(&issued, &response, &context, ms(5_999))
        .expect("fixture response authenticates");
    (state, authenticated, vectors)
}

pub fn authenticated_with_generation(
    generation: PairingGeneration,
) -> (UnlockState, AuthenticatedResponse, Vectors) {
    let vectors = Vectors::load();
    let (state, request) = challenge_request(&vectors);
    let mut entropy = vectors.bytes("mac_ephemeral_private_key_test_only");
    entropy.extend(vectors.bytes("mac_nonce"));
    let mut rng = FixedRandom::new(entropy);
    let mut signer = TestMacSigner::from_vectors(&vectors);
    let issued = issue_challenge(
        request,
        IssueParameters::new(
            vectors.mac_id(),
            vectors.device_id(),
            generation,
            vectors.u64("counter_floor"),
            ms(vectors.u64("issued_at_ms")),
            PublicKeyBytes::try_new(vectors.array("mac_signing_public_key")).unwrap(),
        ),
        &mut rng,
        &mut signer,
    )
    .unwrap();
    let response = response_for_counter_and_generation(&issued, &vectors, 42, 42, generation);
    let paired = PairedDevice::new(
        vectors.mac_id(),
        vectors.device_id(),
        generation,
        PublicKeyBytes::try_new(vectors.array("phone_signing_public_key")).unwrap(),
    );
    let context = VerificationContext::new(vectors.binding(), paired);
    let authenticated = verify_response(&issued, &response, &context, ms(5_999)).unwrap();
    (state, authenticated, vectors)
}

pub fn response_for_counter(
    issued: &IssuedChallenge,
    vectors: &Vectors,
    counter: u64,
    nonce_tweak: u8,
) -> [u8; RESPONSE_FRAME_LEN] {
    response_for_counter_and_generation(issued, vectors, counter, nonce_tweak, vectors.generation())
}

pub fn response_for_counter_and_generation(
    issued: &IssuedChallenge,
    vectors: &Vectors,
    counter: u64,
    nonce_tweak: u8,
    generation: PairingGeneration,
) -> [u8; RESPONSE_FRAME_LEN] {
    if counter <= vectors.u64("counter_floor") {
        let mut structurally_stale: [u8; RESPONSE_FRAME_LEN] =
            vectors.bytes("response_frame").try_into().unwrap();
        structurally_stale[44..52].copy_from_slice(&generation.get().to_be_bytes());
        structurally_stale[76..84].copy_from_slice(&counter.to_be_bytes());
        return structurally_stale;
    }

    let paired_mac = PairedMac::new(
        vectors.mac_id(),
        vectors.device_id(),
        generation,
        PublicKeyBytes::try_new(vectors.array("mac_signing_public_key")).unwrap(),
    );
    let authenticated_challenge = verify_mac_challenge(issued.frame(), &paired_mac).unwrap();
    let mut phone_nonce = vectors.bytes("phone_nonce");
    phone_nonce[0] = 0x40 ^ nonce_tweak ^ 42;
    let mut entropy = vectors.bytes("phone_ephemeral_private_key_test_only");
    entropy.extend(phone_nonce);
    let mut rng = FixedRandom::new(entropy);
    let mut signer = TestPhoneSigner::from_vectors(vectors);
    build_phone_response(
        authenticated_challenge,
        counter.checked_sub(1).unwrap(),
        &mut rng,
        &mut signer,
    )
    .unwrap()
}

pub fn resign_frame(
    issued: &IssuedChallenge,
    vectors: &Vectors,
    frame: &mut [u8; RESPONSE_FRAME_LEN],
) {
    let mut hasher = Sha256::new();
    hasher.update(b"repose-unlock-v1 signature phone-to-mac");
    hasher.update(issued.frame());
    hasher.update(&frame[..RESPONSE_SIGNATURE_OFFSET]);
    let digest: [u8; 32] = hasher.finalize().into();
    let signing_key =
        SigningKey::from_slice(&vectors.bytes("phone_signing_private_key_test_only")).unwrap();
    let signature: Signature = signing_key.sign_prehash(&digest).unwrap();
    let signature = signature.normalize_s().unwrap_or(signature);
    frame[RESPONSE_SIGNATURE_OFFSET..].copy_from_slice(signature.to_bytes().as_slice());
}

pub fn permit_at(reducer_now: u64) -> Permit {
    use repose_unlock_core::replay::{DurableReplayGuard, MemoryCounterStore, ReplayPolicy};

    let (state, authenticated, _) = authenticated();
    let guard = DurableReplayGuard::new(MemoryCounterStore::new(), ReplayPolicy::default());
    let committed = guard.commit(authenticated).expect("durable replay commit");
    let proof = guard.finalize(committed).expect("finalize durable proof");
    let (_, effects) = transition(state, Event::ChallengeVerified(proof), ms(reducer_now)).unwrap();
    let [effect] = effects.try_into().expect("one permit effect");
    let Effect::CreatePermit(permit) = effect else {
        panic!("expected create permit effect");
    };
    permit
}

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "hex length");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(text, 16).unwrap()
        })
        .collect()
}

pub fn take_challenge_request(effects: Vec<Effect>) -> ChallengeRequest {
    let [effect] = effects.try_into().expect("start challenge effect");
    match effect {
        Effect::StartChallenge(request) => request,
        _ => panic!("expected start challenge effect"),
    }
}
