mod common;

use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey};
use repose_unlock_core::domain::MonoMillis;
use repose_unlock_core::phone::{MemoryPhoneResponseStore, PhoneResponseCoordinator};
use repose_unlock_core::protocol::crypto::{
    ChallengeVerificationError, CryptoError, IdentitySigningError, IssueError, IssueParameters,
    MacChallengeSigner, MacChallengeSigningRequest, PairedDevice, PairedMac, PhoneResponseSigner,
    PhoneResponseSigningRequest, TestVectorExpectations, VerificationContext, VerificationError,
    issue_challenge, verify_mac_challenge, verify_response, verify_test_vector,
};
use repose_unlock_core::protocol::messages::{MacId, PublicKeyBytes};
use repose_unlock_core::protocol::wire::{
    CHALLENGE_FRAME_LEN, CHALLENGE_MAC_PUBLIC_KEY_OFFSET, CHALLENGE_PAYLOAD_LEN,
    CHALLENGE_SIGNATURE_OFFSET, HEADER_LEN, MAX_FRAME_LEN, RESPONSE_CIPHERTEXT_OFFSET,
    RESPONSE_FRAME_LEN, RESPONSE_MAC_PUBLIC_KEY_OFFSET, RESPONSE_PAYLOAD_LEN,
    RESPONSE_PHONE_PUBLIC_KEY_OFFSET, RESPONSE_SIGNATURE_OFFSET, RESPONSE_TAG_OFFSET, WireError,
    decode_challenge, decode_response, encode_challenge,
};

use common::{
    FixedRandom, Vectors, challenge_request, fixture_challenge, issued, ms, resign_frame,
    response_for_counter,
};

#[test]
fn committed_challenge_is_independent_byte_for_byte_golden_truth() {
    let (state, issued, vectors) = issued();
    let binary = fixture_challenge();
    assert_eq!(binary.len(), CHALLENGE_FRAME_LEN);
    assert_eq!(binary, vectors.bytes("challenge_frame"));
    assert_eq!(issued.frame(), binary.as_slice());
    assert_eq!(encode_challenge(issued.message()), binary.as_slice());

    let parsed = decode_challenge(&binary).unwrap();
    assert_eq!(parsed.mac_id(), vectors.mac_id());
    assert_eq!(parsed.device_id(), vectors.device_id());
    assert_eq!(parsed.pairing_generation(), vectors.generation());
    assert_eq!(parsed.binding(), vectors.binding());
    assert_eq!(parsed.challenge_id().get(), vectors.u64("challenge_id"));
    assert_eq!(parsed.counter_floor(), vectors.u64("counter_floor"));
    assert_eq!(parsed.ttl_ms(), vectors.u64("ttl_ms") as u32);
    assert_eq!(parsed.mac_nonce(), &vectors.array::<32>("mac_nonce"));
    assert_eq!(
        parsed.mac_ephemeral_public_key().as_bytes(),
        &vectors.array::<65>("mac_ephemeral_public_key")
    );
    assert_eq!(
        parsed.mac_identity_signature(),
        &vectors.array::<64>("mac_challenge_signature_raw_low_s")
    );
    assert_eq!(issued.issued_at(), ms(vectors.u64("issued_at_ms")));
    assert_eq!(issued.deadline(), ms(vectors.u64("deadline_ms")));
    assert_eq!(state.challenge_id(), Some(parsed.challenge_id()));
}

#[test]
fn all_documented_lengths_and_offsets_are_fixed() {
    assert_eq!(HEADER_LEN, 12);
    assert_eq!(CHALLENGE_PAYLOAD_LEN, 237);
    assert_eq!(CHALLENGE_FRAME_LEN, 249);
    assert_eq!(RESPONSE_PAYLOAD_LEN, 378);
    assert_eq!(RESPONSE_FRAME_LEN, 390);
    assert_eq!(MAX_FRAME_LEN, RESPONSE_FRAME_LEN);
    assert_eq!(CHALLENGE_MAC_PUBLIC_KEY_OFFSET, 120);
    assert_eq!(RESPONSE_MAC_PUBLIC_KEY_OFFSET, 148);
    assert_eq!(RESPONSE_PHONE_PUBLIC_KEY_OFFSET, 213);
    assert_eq!(RESPONSE_CIPHERTEXT_OFFSET, 278);
    assert_eq!(RESPONSE_TAG_OFFSET, 310);
    assert_eq!(RESPONSE_SIGNATURE_OFFSET, 326);

    let vectors = Vectors::load();
    let challenge = fixture_challenge();
    assert_eq!(&challenge[0..4], b"RPUK");
    assert_eq!(&challenge[4..12], &[1, 1, 0, 0, 0, 0, 0, 237]);
    assert_eq!(&challenge[12..28], vectors.bytes("mac_id"));
    assert_eq!(&challenge[28..44], vectors.bytes("device_id"));
    assert_eq!(&challenge[44..52], &7_u64.to_be_bytes());
    assert_eq!(&challenge[52..56], &501_u32.to_be_bytes());
    assert_eq!(&challenge[56..60], &0x1234_5678_u32.to_be_bytes());
    assert_eq!(
        &challenge[CHALLENGE_MAC_PUBLIC_KEY_OFFSET..CHALLENGE_SIGNATURE_OFFSET],
        vectors.bytes("mac_ephemeral_public_key")
    );
    assert_eq!(CHALLENGE_SIGNATURE_OFFSET, 185);
    assert_eq!(
        &challenge[CHALLENGE_SIGNATURE_OFFSET..],
        vectors.bytes("mac_challenge_signature_raw_low_s")
    );

    let response = vectors.bytes("response_frame");
    assert_eq!(&response[4..12], &[1, 2, 0, 0, 0, 0, 1, 122]);
    assert_eq!(&response[44..52], &7_u64.to_be_bytes());
    assert_eq!(&response[76..84], &42_u64.to_be_bytes());
    assert_eq!(
        &response[RESPONSE_CIPHERTEXT_OFFSET..RESPONSE_TAG_OFFSET],
        vectors.bytes("response_ciphertext")
    );
    assert_eq!(
        &response[RESPONSE_TAG_OFFSET..RESPONSE_SIGNATURE_OFFSET],
        vectors.bytes("response_tag")
    );
    assert_eq!(
        &response[RESPONSE_SIGNATURE_OFFSET..],
        vectors.bytes("response_signature_raw_low_s")
    );
}

#[test]
fn ecdh_hkdf_aead_signature_and_plaintext_match_committed_vectors() {
    let (_, issued, vectors) = issued();
    let response_bytes = vectors.bytes("response_frame");
    let mac_challenge_signature_hash = vectors.array("mac_challenge_signature_hash");
    let shared_secret = vectors.array("ecdh_shared_secret");
    let context_hash = vectors.array("kdf_context_hash");
    let salt = vectors.array("hkdf_salt");
    let mac_key = vectors.array("mac_to_phone_key");
    let mac_nonce = vectors.array("mac_to_phone_nonce");
    let phone_key = vectors.array("phone_to_mac_key");
    let phone_nonce = vectors.array("phone_to_mac_nonce");
    let aad = vectors.bytes("response_aad");
    let plaintext = vectors.array("response_plaintext");
    let signature_hash = vectors.array("signature_transcript_hash");
    verify_test_vector(
        &issued,
        &response_bytes,
        &TestVectorExpectations {
            mac_challenge_signature_hash: &mac_challenge_signature_hash,
            shared_secret: &shared_secret,
            context_hash: &context_hash,
            hkdf_salt: &salt,
            mac_to_phone_key: &mac_key,
            mac_to_phone_nonce: &mac_nonce,
            phone_to_mac_key: &phone_key,
            phone_to_mac_nonce: &phone_nonce,
            response_aad: &aad,
            response_plaintext: &plaintext,
            response_signature_hash: &signature_hash,
        },
    )
    .unwrap();
    assert_ne!(mac_key, phone_key);
    assert_ne!(mac_nonce, phone_nonce);

    let context = VerificationContext::new(vectors.binding(), vectors.pairing());
    let authenticated = verify_response(&issued, &response_bytes, &context, ms(5_999)).unwrap();
    assert_eq!(authenticated.mac_id(), vectors.mac_id());
    assert_eq!(authenticated.device_id(), vectors.device_id());
    assert_eq!(authenticated.pairing_generation(), vectors.generation());
    assert_eq!(authenticated.binding(), vectors.binding());
    assert_eq!(authenticated.challenge_id().get(), 1);
    assert_eq!(authenticated.counter(), 42);
}

#[test]
fn challenge_parser_rejects_every_noncanonical_frame_class() {
    let valid = fixture_challenge();
    let mut mutations = Vec::new();
    let mut bad_magic = valid.clone();
    bad_magic[0] ^= 1;
    mutations.push(bad_magic);
    let mut bad_version = valid.clone();
    bad_version[4] = 2;
    mutations.push(bad_version);
    let mut bad_kind = valid.clone();
    bad_kind[5] = 2;
    mutations.push(bad_kind);
    let mut unknown_kind = valid.clone();
    unknown_kind[5] = 0xff;
    mutations.push(unknown_kind);
    let mut bad_flags = valid.clone();
    bad_flags[7] = 1;
    mutations.push(bad_flags);
    let mut short_length = valid.clone();
    short_length[11] -= 1;
    mutations.push(short_length);
    let mut over_max = valid.clone();
    over_max[8..12].copy_from_slice(&379_u32.to_be_bytes());
    mutations.push(over_max);
    let mut bad_prefix = valid.clone();
    bad_prefix[CHALLENGE_MAC_PUBLIC_KEY_OFFSET] = 3;
    mutations.push(bad_prefix);
    let mut off_curve = valid.clone();
    off_curve[CHALLENGE_MAC_PUBLIC_KEY_OFFSET..].fill(0);
    off_curve[CHALLENGE_MAC_PUBLIC_KEY_OFFSET] = 4;
    mutations.push(off_curve);
    let mut zero_ttl = valid.clone();
    zero_ttl[84..88].fill(0);
    mutations.push(zero_ttl);
    let mut excessive_ttl = valid.clone();
    excessive_ttl[84..88].copy_from_slice(&60_001_u32.to_be_bytes());
    mutations.push(excessive_ttl);

    for mutation in mutations {
        assert!(decode_challenge(&mutation).is_err());
    }
    assert!(decode_challenge(&valid[..valid.len() - 1]).is_err());
    let mut trailing = valid;
    trailing.push(0);
    assert_eq!(decode_challenge(&trailing), Err(WireError::TrailingBytes));
}

#[test]
fn response_parser_rejects_header_length_trailing_and_invalid_points() {
    let vectors = Vectors::load();
    let valid = vectors.bytes("response_frame");
    for (offset, value) in [(0, valid[0] ^ 1), (4, 2), (5, 1), (5, 0xff), (7, 1)] {
        let mut mutated = valid.clone();
        mutated[offset] = value;
        assert!(decode_response(&mutated).is_err(), "offset {offset}");
    }
    let mut wrong_length = valid.clone();
    wrong_length[8..12].copy_from_slice(&377_u32.to_be_bytes());
    assert!(decode_response(&wrong_length).is_err());
    let mut over_max = valid.clone();
    over_max[8..12].copy_from_slice(&379_u32.to_be_bytes());
    assert!(decode_response(&over_max).is_err());
    assert!(decode_response(&valid[..valid.len() - 1]).is_err());
    let mut trailing = valid.clone();
    trailing.push(0);
    assert_eq!(decode_response(&trailing), Err(WireError::TrailingBytes));

    for key_offset in [
        RESPONSE_MAC_PUBLIC_KEY_OFFSET,
        RESPONSE_PHONE_PUBLIC_KEY_OFFSET,
    ] {
        let mut bad_prefix = valid.clone();
        bad_prefix[key_offset] = 2;
        assert!(decode_response(&bad_prefix).is_err());
        let mut off_curve = valid.clone();
        off_curve[key_offset..key_offset + 65].fill(0);
        off_curve[key_offset] = 4;
        assert!(decode_response(&off_curve).is_err());
    }
}

#[test]
fn every_security_relevant_response_class_is_authenticated() {
    let (_, issued, vectors) = issued();
    let valid = vectors.bytes("response_frame");
    let context = VerificationContext::new(vectors.binding(), vectors.pairing());
    let alternate_point = vectors.bytes("phone_signing_public_key");
    let cases: &[(&str, usize)] = &[
        ("mac id", 12),
        ("device id", 28),
        ("pairing generation", 44),
        ("uid", 52),
        ("audit session", 56),
        ("lock epoch", 60),
        ("challenge id", 68),
        ("counter", 76),
        ("mac nonce", 84),
        ("phone nonce", 116),
        ("ciphertext", RESPONSE_CIPHERTEXT_OFFSET),
        ("tag", RESPONSE_TAG_OFFSET),
        ("signature", RESPONSE_SIGNATURE_OFFSET),
    ];
    for (name, offset) in cases {
        let mut mutated = valid.clone();
        mutated[*offset] ^= 1;
        assert!(
            verify_response(&issued, &mutated, &context, ms(5_999)).is_err(),
            "mutation unexpectedly authenticated: {name}"
        );
    }
    for (name, offset) in [
        ("mirrored Mac public key", RESPONSE_MAC_PUBLIC_KEY_OFFSET),
        ("phone public key", RESPONSE_PHONE_PUBLIC_KEY_OFFSET),
    ] {
        let mut mutated = valid.clone();
        mutated[offset..offset + 65].copy_from_slice(&alternate_point);
        assert!(
            verify_response(&issued, &mutated, &context, ms(5_999)).is_err(),
            "mutation unexpectedly authenticated: {name}"
        );
    }
}

#[test]
fn local_context_counter_and_deadline_are_fail_closed_at_boundaries() {
    let (_, issued, vectors) = issued();
    let response = vectors.bytes("response_frame");
    let valid_context = VerificationContext::new(vectors.binding(), vectors.pairing());
    assert!(verify_response(&issued, &response, &valid_context, ms(5_999)).is_ok());
    assert_eq!(
        verify_response(&issued, &response, &valid_context, ms(6_000)).unwrap_err(),
        VerificationError::Expired
    );
    assert_eq!(
        verify_response(&issued, &response, &valid_context, ms(6_001)).unwrap_err(),
        VerificationError::Expired
    );
    assert_eq!(
        verify_response(&issued, &response, &valid_context, ms(999)).unwrap_err(),
        VerificationError::NonMonotonicTime
    );

    let stale = response_for_counter(&issued, &vectors, 41, 1);
    assert_eq!(
        verify_response(&issued, &stale, &valid_context, ms(5_999)).unwrap_err(),
        VerificationError::CounterNotAdvanced
    );

    let wrong_mac = PairedDevice::new(
        MacId::new([9; 16]),
        vectors.device_id(),
        vectors.generation(),
        PublicKeyBytes::try_new(vectors.array("phone_signing_public_key")).unwrap(),
    );
    let wrong_pairing = VerificationContext::new(vectors.binding(), wrong_mac);
    assert_eq!(
        verify_response(&issued, &response, &wrong_pairing, ms(5_999)).unwrap_err(),
        VerificationError::PairingMismatch
    );

    let wrong_identity_key = PairedDevice::new(
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
        PublicKeyBytes::try_new(vectors.array("mac_ephemeral_public_key")).unwrap(),
    );
    let wrong_key_context = VerificationContext::new(vectors.binding(), wrong_identity_key);
    assert_eq!(
        verify_response(&issued, &response, &wrong_key_context, ms(5_999)).unwrap_err(),
        VerificationError::Signature
    );
}

#[test]
fn malformed_and_high_s_signatures_are_rejected_before_verification() {
    let (_, issued, vectors) = issued();
    let valid = vectors.bytes("response_frame");
    let context = VerificationContext::new(vectors.binding(), vectors.pairing());

    let mut r_zero = valid.clone();
    r_zero[RESPONSE_SIGNATURE_OFFSET..RESPONSE_SIGNATURE_OFFSET + 32].fill(0);
    assert_eq!(
        verify_response(&issued, &r_zero, &context, ms(5_999)).unwrap_err(),
        VerificationError::InvalidSignatureEncoding
    );
    let mut s_zero = valid.clone();
    s_zero[RESPONSE_SIGNATURE_OFFSET + 32..].fill(0);
    assert_eq!(
        verify_response(&issued, &s_zero, &context, ms(5_999)).unwrap_err(),
        VerificationError::InvalidSignatureEncoding
    );
    let mut out_of_range = valid.clone();
    out_of_range[RESPONSE_SIGNATURE_OFFSET..RESPONSE_SIGNATURE_OFFSET + 32].fill(0xff);
    assert_eq!(
        verify_response(&issued, &out_of_range, &context, ms(5_999)).unwrap_err(),
        VerificationError::InvalidSignatureEncoding
    );
    let mut s_out_of_range = valid.clone();
    s_out_of_range[RESPONSE_SIGNATURE_OFFSET + 32..].fill(0xff);
    assert_eq!(
        verify_response(&issued, &s_out_of_range, &context, ms(5_999)).unwrap_err(),
        VerificationError::InvalidSignatureEncoding
    );

    const ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63,
        0x25, 0x51,
    ];
    let low_s: [u8; 32] = valid[RESPONSE_SIGNATURE_OFFSET + 32..].try_into().unwrap();
    let high_s = subtract_be(ORDER, low_s);
    let mut twin = valid;
    twin[RESPONSE_SIGNATURE_OFFSET + 32..].copy_from_slice(&high_s);
    assert_eq!(
        verify_response(&issued, &twin, &context, ms(5_999)).unwrap_err(),
        VerificationError::HighS
    );
}

#[test]
fn valid_signature_cannot_rescue_bad_aead_or_changed_aad() {
    let (_, issued, vectors) = issued();
    let context = VerificationContext::new(vectors.binding(), vectors.pairing());

    let mut bad_tag: [u8; RESPONSE_FRAME_LEN] = vectors.bytes("response_frame").try_into().unwrap();
    bad_tag[RESPONSE_TAG_OFFSET] ^= 1;
    resign_frame(&issued, &vectors, &mut bad_tag);
    assert_eq!(
        verify_response(&issued, &bad_tag, &context, ms(5_999)).unwrap_err(),
        VerificationError::Crypto(CryptoError::Aead)
    );

    let mut changed_aad: [u8; RESPONSE_FRAME_LEN] =
        vectors.bytes("response_frame").try_into().unwrap();
    changed_aad[116] ^= 1;
    resign_frame(&issued, &vectors, &mut changed_aad);
    assert_eq!(
        verify_response(&issued, &changed_aad, &context, ms(5_999)).unwrap_err(),
        VerificationError::Crypto(CryptoError::Aead)
    );
}

#[test]
fn recomputed_aead_after_cleartext_mutation_still_fails_old_signature() {
    let (_, issued, vectors) = issued();
    let authenticated_challenge =
        verify_mac_challenge(issued.frame(), &vectors.paired_mac()).unwrap();
    let mut entropy = vectors.bytes("phone_ephemeral_private_key_test_only");
    let mut changed_nonce = vectors.bytes("phone_nonce");
    changed_nonce[0] ^= 1;
    entropy.extend(changed_nonce);
    let mut rng = FixedRandom::new(entropy);
    let mut signer = P256PhoneSigner(
        SigningKey::from_slice(&vectors.bytes("phone_signing_private_key_test_only")).unwrap(),
    );
    let coordinator = PhoneResponseCoordinator::new(MemoryPhoneResponseStore::new());
    coordinator
        .rotate_pairing(vectors.mac_id(), vectors.device_id(), vectors.generation())
        .unwrap();
    let mut frame = coordinator
        .respond(authenticated_challenge, &mut rng, &mut signer)
        .expect("build independently authenticated mutation");
    frame[RESPONSE_SIGNATURE_OFFSET..]
        .copy_from_slice(&vectors.bytes("response_signature_raw_low_s"));

    let context = VerificationContext::new(vectors.binding(), vectors.pairing());
    assert_eq!(
        verify_response(&issued, &frame, &context, MonoMillis::new(5_999)).unwrap_err(),
        VerificationError::Signature
    );
}

fn subtract_be(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut result = [0_u8; 32];
    let mut borrow = 0_u16;
    for index in (0..32).rev() {
        let lhs = u16::from(left[index]);
        let rhs = u16::from(right[index]) + borrow;
        if lhs >= rhs {
            result[index] = (lhs - rhs) as u8;
            borrow = 0;
        } else {
            result[index] = (lhs + 256 - rhs) as u8;
            borrow = 1;
        }
    }
    assert_eq!(borrow, 0);
    result
}

#[test]
fn phone_authenticates_the_committed_challenge_with_its_paired_mac_record() {
    let vectors = Vectors::load();
    let frame = fixture_challenge();
    let verified = verify_mac_challenge(&frame, &vectors.paired_mac()).unwrap();

    assert_eq!(verified.mac_id(), vectors.mac_id());
    assert_eq!(verified.device_id(), vectors.device_id());
    assert_eq!(verified.pairing_generation(), vectors.generation());
    assert_eq!(verified.binding(), vectors.binding());
    assert_eq!(verified.challenge_id().get(), vectors.u64("challenge_id"));
    assert_eq!(
        &frame[CHALLENGE_SIGNATURE_OFFSET..],
        vectors.bytes("mac_challenge_signature_raw_low_s")
    );
}

#[test]
fn forged_or_mutated_challenge_never_becomes_phone_authenticated() {
    let vectors = Vectors::load();
    let valid = fixture_challenge();
    let paired = vectors.paired_mac();

    for offset in [0, 4, 5, 7, 11, 12, 28, 44, 52, 56, 60, 68, 76, 84, 88, 120] {
        let mut mutated = valid.clone();
        mutated[offset] ^= 1;
        assert!(
            verify_mac_challenge(&mutated, &paired).is_err(),
            "challenge mutation unexpectedly authenticated at offset {offset}"
        );
    }

    let wrong_key = PairedMac::new(
        vectors.mac_id(),
        vectors.device_id(),
        vectors.generation(),
        PublicKeyBytes::try_new(vectors.array("phone_signing_public_key")).unwrap(),
        PublicKeyBytes::try_new(vectors.array("phone_signing_public_key")).unwrap(),
    );
    assert_eq!(
        verify_mac_challenge(&valid, &wrong_key).err().unwrap(),
        ChallengeVerificationError::Signature
    );
}

#[test]
fn malformed_and_high_s_mac_challenge_signatures_are_rejected() {
    let vectors = Vectors::load();
    let valid = fixture_challenge();
    let paired = vectors.paired_mac();

    for (name, range) in [
        (
            "r zero",
            CHALLENGE_SIGNATURE_OFFSET..CHALLENGE_SIGNATURE_OFFSET + 32,
        ),
        (
            "s zero",
            CHALLENGE_SIGNATURE_OFFSET + 32..CHALLENGE_SIGNATURE_OFFSET + 64,
        ),
    ] {
        let mut malformed = valid.clone();
        malformed[range].fill(0);
        assert_eq!(
            verify_mac_challenge(&malformed, &paired).err().unwrap(),
            ChallengeVerificationError::InvalidSignatureEncoding,
            "{name}"
        );
    }

    for range in [
        CHALLENGE_SIGNATURE_OFFSET..CHALLENGE_SIGNATURE_OFFSET + 32,
        CHALLENGE_SIGNATURE_OFFSET + 32..CHALLENGE_SIGNATURE_OFFSET + 64,
    ] {
        let mut malformed = valid.clone();
        malformed[range].fill(0xff);
        assert_eq!(
            verify_mac_challenge(&malformed, &paired).err().unwrap(),
            ChallengeVerificationError::InvalidSignatureEncoding
        );
    }

    const ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63,
        0x25, 0x51,
    ];
    let low_s: [u8; 32] = valid[CHALLENGE_SIGNATURE_OFFSET + 32..].try_into().unwrap();
    let mut high_s = valid;
    high_s[CHALLENGE_SIGNATURE_OFFSET + 32..].copy_from_slice(&subtract_be(ORDER, low_s));
    assert_eq!(
        verify_mac_challenge(&high_s, &paired).err().unwrap(),
        ChallengeVerificationError::HighS
    );
}

struct P256MacSigner {
    key: SigningKey,
    calls: usize,
    last_prehash: Option<[u8; 32]>,
}

impl MacChallengeSigner for P256MacSigner {
    fn sign_challenge(
        &mut self,
        request: &MacChallengeSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError> {
        self.calls += 1;
        self.last_prehash = Some(*request.prehash());
        let signature: Signature = self
            .key
            .sign_prehash(request.prehash())
            .map_err(|_| IdentitySigningError)?;
        let signature = signature.normalize_s().unwrap_or(signature);
        Ok(signature.to_bytes().into())
    }
}

struct P256PhoneSigner(SigningKey);

impl PhoneResponseSigner for P256PhoneSigner {
    fn sign_response(
        &mut self,
        request: &PhoneResponseSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError> {
        let signature: Signature = self
            .0
            .sign_prehash(request.prehash())
            .map_err(|_| IdentitySigningError)?;
        let signature = signature.normalize_s().unwrap_or(signature);
        Ok(signature.to_bytes().into())
    }
}

struct FixedMacSigner {
    signature: [u8; 64],
    calls: usize,
}

impl MacChallengeSigner for FixedMacSigner {
    fn sign_challenge(
        &mut self,
        _request: &MacChallengeSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError> {
        self.calls += 1;
        Ok(self.signature)
    }
}

#[test]
fn issuer_uses_only_the_purpose_specific_mac_signer_and_validates_its_output() {
    let vectors = Vectors::load();
    let (_, request) = challenge_request(&vectors);
    let mac_key =
        SigningKey::from_slice(&vectors.bytes("mac_signing_private_key_test_only")).unwrap();
    let mut signer = P256MacSigner {
        key: mac_key,
        calls: 0,
        last_prehash: None,
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
            PublicKeyBytes::try_new(vectors.array("mac_signing_public_key")).unwrap(),
        ),
        &mut rng,
        &mut signer,
    )
    .unwrap();
    assert_eq!(signer.calls, 1);
    assert_eq!(
        signer.last_prehash.unwrap(),
        vectors.array::<32>("mac_challenge_signature_hash")
    );
    assert_eq!(issued.frame(), fixture_challenge().as_slice());

    let (_, invalid_request) = challenge_request(&vectors);
    let mut never_called = FixedMacSigner {
        signature: [0; 64],
        calls: 0,
    };
    let mut no_entropy = FixedRandom::new(Vec::new());
    assert_eq!(
        issue_challenge(
            invalid_request,
            IssueParameters::new(
                vectors.mac_id(),
                vectors.device_id(),
                vectors.generation(),
                vectors.u64("counter_floor"),
                ms(vectors.u64("deadline_ms")),
                PublicKeyBytes::try_new(vectors.array("mac_signing_public_key")).unwrap(),
            ),
            &mut no_entropy,
            &mut never_called,
        )
        .err()
        .unwrap(),
        IssueError::NonPositiveLifetime
    );
    assert_eq!(never_called.calls, 0);

    let (_, request) = challenge_request(&vectors);
    let mut invalid_signature = FixedMacSigner {
        signature: [0; 64],
        calls: 0,
    };
    let mut entropy = vectors.bytes("mac_ephemeral_private_key_test_only");
    entropy.extend(vectors.bytes("mac_nonce"));
    let mut rng = FixedRandom::new(entropy);
    assert_eq!(
        issue_challenge(
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
            &mut invalid_signature,
        )
        .err()
        .unwrap(),
        IssueError::InvalidSignatureEncoding
    );

    const ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63,
        0x25, 0x51,
    ];
    let mut high_s = vectors.array::<64>("mac_challenge_signature_raw_low_s");
    let low_s: [u8; 32] = high_s[32..].try_into().unwrap();
    high_s[32..].copy_from_slice(&subtract_be(ORDER, low_s));
    let (_, request) = challenge_request(&vectors);
    let mut high_s_signer = FixedMacSigner {
        signature: high_s,
        calls: 0,
    };
    let mut entropy = vectors.bytes("mac_ephemeral_private_key_test_only");
    entropy.extend(vectors.bytes("mac_nonce"));
    let mut rng = FixedRandom::new(entropy);
    assert_eq!(
        issue_challenge(
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
            &mut high_s_signer,
        )
        .err()
        .unwrap(),
        IssueError::HighS
    );

    let (_, request) = challenge_request(&vectors);
    let mut wrong_key_signer = P256MacSigner {
        key: SigningKey::from_slice(&vectors.bytes("phone_signing_private_key_test_only")).unwrap(),
        calls: 0,
        last_prehash: None,
    };
    let mut entropy = vectors.bytes("mac_ephemeral_private_key_test_only");
    entropy.extend(vectors.bytes("mac_nonce"));
    let mut rng = FixedRandom::new(entropy);
    assert_eq!(
        issue_challenge(
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
            &mut wrong_key_signer,
        )
        .err()
        .unwrap(),
        IssueError::Signature
    );
}

#[test]
fn reference_phone_coordinator_requires_a_mac_authenticated_challenge() {
    let (_, issued, vectors) = issued();
    let authenticated_challenge =
        verify_mac_challenge(&fixture_challenge(), &vectors.paired_mac()).unwrap();
    let mut entropy = vectors.bytes("phone_ephemeral_private_key_test_only");
    entropy.extend(vectors.bytes("phone_nonce"));
    let mut rng = FixedRandom::new(entropy);
    let mut signer = P256PhoneSigner(
        SigningKey::from_slice(&vectors.bytes("phone_signing_private_key_test_only")).unwrap(),
    );
    let coordinator = PhoneResponseCoordinator::new(MemoryPhoneResponseStore::new());
    coordinator
        .rotate_pairing(vectors.mac_id(), vectors.device_id(), vectors.generation())
        .unwrap();
    let response = coordinator
        .respond(authenticated_challenge, &mut rng, &mut signer)
        .expect("authenticated challenge builds response");
    assert_eq!(response, vectors.bytes("response_frame").as_slice());

    let context = VerificationContext::new(vectors.binding(), vectors.pairing());
    verify_response(&issued, &response, &context, ms(5_999)).unwrap();
}
