#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey};
use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};
use repose_unlock_core::protocol::crypto::{
    AuthenticatedResponse, CryptoRandom, IssueParameters, IssuedChallenge, PairedDevice,
    RandomError, VerificationContext, derive_session_material, issue_challenge, response_aad,
    response_plaintext, signature_transcript_hash, verify_response,
};
use repose_unlock_core::protocol::messages::{DeviceId, MacId, PairingGeneration, PublicKeyBytes};
use repose_unlock_core::protocol::wire::{
    RESPONSE_CIPHERTEXT_OFFSET, RESPONSE_FRAME_LEN, RESPONSE_SIGNATURE_OFFSET, RESPONSE_TAG_OFFSET,
    decode_response,
};
use repose_unlock_core::state_machine::{
    ChallengeRequest, Effect, Event, Permit, SessionBinding, TimingPolicy, UnlockState, transition,
};
use serde_json::Value;

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

    let mut entropy = vectors.bytes("mac_ephemeral_private_key_test_only");
    entropy.extend(vectors.bytes("mac_nonce"));
    let mut rng = FixedRandom::new(entropy);
    let issued = issue_challenge(
        request,
        IssueParameters::new(
            vectors.mac_id(),
            vectors.device_id(),
            vectors.generation(),
            vectors.u64("counter_floor"),
            ms(vectors.u64("issued_at_ms")),
        ),
        &mut rng,
    )
    .expect("issue fixture challenge");
    (challenging, issued, vectors)
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

pub fn response_for_counter(
    issued: &IssuedChallenge,
    vectors: &Vectors,
    counter: u64,
    nonce_tweak: u8,
) -> [u8; RESPONSE_FRAME_LEN] {
    let mut frame: [u8; RESPONSE_FRAME_LEN] = vectors.bytes("response_frame").try_into().unwrap();
    frame[76..84].copy_from_slice(&counter.to_be_bytes());
    frame[116] = 0x40 ^ nonce_tweak ^ 42;

    let response = decode_response(&frame).expect("structural response after clear mutation");
    let material = derive_session_material(issued, &response).expect("derive fixture material");
    let aad = response_aad(issued, &response);
    let plaintext = response_plaintext(issued, &response);
    let cipher = Aes256Gcm::new_from_slice(material.phone_to_mac_key()).unwrap();
    let mut ciphertext = plaintext;
    let tag: Tag = cipher
        .encrypt_in_place_detached(
            Nonce::from_slice(material.phone_to_mac_nonce()),
            &aad,
            &mut ciphertext,
        )
        .unwrap();
    frame[RESPONSE_CIPHERTEXT_OFFSET..RESPONSE_TAG_OFFSET].copy_from_slice(&ciphertext);
    frame[RESPONSE_TAG_OFFSET..RESPONSE_SIGNATURE_OFFSET].copy_from_slice(tag.as_slice());

    resign_frame(issued, vectors, &mut frame);
    frame
}

pub fn resign_frame(
    issued: &IssuedChallenge,
    vectors: &Vectors,
    frame: &mut [u8; RESPONSE_FRAME_LEN],
) {
    let response = decode_response(frame).unwrap();
    let digest = signature_transcript_hash(issued, &response);
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
    let proof = committed.into_challenge_verified();
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
