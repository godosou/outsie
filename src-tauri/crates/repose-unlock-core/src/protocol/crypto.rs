use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use hkdf::Hkdf;
use p256::SecretKey;
use p256::ecdh::diffie_hellman;
use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use rand_core::RngCore;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::domain::MonoMillis;
use crate::protocol::messages::{
    Challenge, DeviceId, MacId, PairingGeneration, PublicKeyBytes, Response,
};
use crate::protocol::transcript::{
    MAC_TO_PHONE_KEY_LABEL, MAC_TO_PHONE_NONCE_LABEL, PHONE_TO_MAC_KEY_LABEL,
    PHONE_TO_MAC_NONCE_LABEL,
};
use crate::protocol::{transcript, wire};
use crate::state_machine::{ChallengeId, ChallengeRequest, SessionBinding};

const PRIVATE_KEY_ATTEMPTS: usize = 8;

/// An injected source of cryptographic randomness.
///
/// Production callers should use [`OsRandom`]. Deterministic implementations are
/// only appropriate for fixed interoperability tests.
pub trait CryptoRandom {
    fn fill_bytes(&mut self, output: &mut [u8]) -> Result<(), RandomError>;
}

pub struct OsRandom;

impl CryptoRandom for OsRandom {
    fn fill_bytes(&mut self, output: &mut [u8]) -> Result<(), RandomError> {
        rand_core::OsRng
            .try_fill_bytes(output)
            .map_err(|_| RandomError)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomError;

impl Display for RandomError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("cryptographic random source failed")
    }
}

impl Error for RandomError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentitySigningError;

impl Display for IdentitySigningError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("identity-key signing backend failed")
    }
}

impl Error for IdentitySigningError {}

/// A purpose-scoped request that can only be created by [`issue_challenge`].
///
/// The request exposes the already domain-separated single SHA-256 prehash to a
/// Keychain/Secure Enclave adapter. Callers cannot construct one to obtain an
/// arbitrary-signing oracle.
pub struct MacChallengeSigningRequest<'a> {
    prehash: &'a [u8; 32],
}

impl MacChallengeSigningRequest<'_> {
    #[must_use]
    pub const fn prehash(&self) -> &[u8; 32] {
        self.prehash
    }
}

pub trait MacChallengeSigner {
    fn sign_challenge(
        &mut self,
        request: &MacChallengeSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError>;
}

/// A purpose-scoped request produced only while building a verified response.
pub struct PhoneResponseSigningRequest<'a> {
    prehash: &'a [u8; 32],
}

impl PhoneResponseSigningRequest<'_> {
    #[must_use]
    pub const fn prehash(&self) -> &[u8; 32] {
        self.prehash
    }
}

pub trait PhoneResponseSigner {
    fn sign_response(
        &mut self,
        request: &PhoneResponseSigningRequest<'_>,
    ) -> Result<[u8; 64], IdentitySigningError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueParameters {
    mac_id: MacId,
    device_id: DeviceId,
    pairing_generation: PairingGeneration,
    counter_floor: u64,
    issued_at: MonoMillis,
    mac_identity_public_key: PublicKeyBytes,
}

impl IssueParameters {
    #[must_use]
    pub const fn new(
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
        counter_floor: u64,
        issued_at: MonoMillis,
        mac_identity_public_key: PublicKeyBytes,
    ) -> Self {
        Self {
            mac_id,
            device_id,
            pairing_generation,
            counter_floor,
            issued_at,
            mac_identity_public_key,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairedDevice {
    mac_id: MacId,
    device_id: DeviceId,
    pairing_generation: PairingGeneration,
    phone_identity_public_key: PublicKeyBytes,
}

impl PairedDevice {
    #[must_use]
    pub const fn new(
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
        phone_identity_public_key: PublicKeyBytes,
    ) -> Self {
        Self {
            mac_id,
            device_id,
            pairing_generation,
            phone_identity_public_key,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairedMac {
    mac_id: MacId,
    device_id: DeviceId,
    pairing_generation: PairingGeneration,
    mac_identity_public_key: PublicKeyBytes,
    phone_identity_public_key: PublicKeyBytes,
}

impl PairedMac {
    #[must_use]
    pub const fn new(
        mac_id: MacId,
        device_id: DeviceId,
        pairing_generation: PairingGeneration,
        mac_identity_public_key: PublicKeyBytes,
        phone_identity_public_key: PublicKeyBytes,
    ) -> Self {
        Self {
            mac_id,
            device_id,
            pairing_generation,
            mac_identity_public_key,
            phone_identity_public_key,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationContext {
    authoritative_binding: SessionBinding,
    paired_device: PairedDevice,
}

impl VerificationContext {
    #[must_use]
    pub const fn new(authoritative_binding: SessionBinding, paired_device: PairedDevice) -> Self {
        Self {
            authoritative_binding,
            paired_device,
        }
    }
}

/// A locally issued challenge together with its ephemeral private key and local clock bounds.
///
/// The value deliberately cannot be cloned, copied, or formatted with `Debug`.
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::IssuedChallenge;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<IssuedChallenge>();
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::IssuedChallenge;
/// fn assert_debug<T: std::fmt::Debug>() {}
/// assert_debug::<IssuedChallenge>();
/// ```
pub struct IssuedChallenge {
    message: Challenge,
    frame: [u8; wire::CHALLENGE_FRAME_LEN],
    issued_at: MonoMillis,
    deadline: MonoMillis,
    mac_ephemeral_private_key: Zeroizing<[u8; 32]>,
}

impl IssuedChallenge {
    #[must_use]
    pub const fn message(&self) -> &Challenge {
        &self.message
    }

    #[must_use]
    pub const fn frame(&self) -> &[u8; wire::CHALLENGE_FRAME_LEN] {
        &self.frame
    }

    #[must_use]
    pub const fn issued_at(&self) -> MonoMillis {
        self.issued_at
    }

    #[must_use]
    pub const fn deadline(&self) -> MonoMillis {
        self.deadline
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueError {
    NonPositiveLifetime,
    LifetimeTooLong,
    Randomness,
    InvalidPrivateKeyEntropy,
    Signing,
    InvalidSignatureEncoding,
    HighS,
    Signature,
}

impl Display for IssueError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot issue v1 challenge: {self:?}")
    }
}

impl Error for IssueError {}

pub fn issue_challenge<R: CryptoRandom, S: MacChallengeSigner>(
    request: ChallengeRequest,
    parameters: IssueParameters,
    rng: &mut R,
    signer: &mut S,
) -> Result<IssuedChallenge, IssueError> {
    let ttl = request
        .deadline()
        .get()
        .checked_sub(parameters.issued_at.get())
        .ok_or(IssueError::NonPositiveLifetime)?;
    if ttl == 0 {
        return Err(IssueError::NonPositiveLifetime);
    }
    if ttl > crate::state_machine::TimingPolicy::MAX_CHALLENGE_TTL_MS {
        return Err(IssueError::LifetimeTooLong);
    }
    let ttl_ms = u32::try_from(ttl).map_err(|_| IssueError::LifetimeTooLong)?;

    let mut private_bytes = Zeroizing::new([0_u8; 32]);
    let mut private_key = None;
    for _ in 0..PRIVATE_KEY_ATTEMPTS {
        rng.fill_bytes(private_bytes.as_mut())
            .map_err(|_| IssueError::Randomness)?;
        if let Ok(candidate) = SecretKey::from_slice(private_bytes.as_ref()) {
            private_key = Some(candidate);
            break;
        }
    }
    let private_key = private_key.ok_or(IssueError::InvalidPrivateKeyEntropy)?;
    let encoded_public = private_key.public_key().to_encoded_point(false);
    let public_bytes: [u8; 65] = encoded_public
        .as_bytes()
        .try_into()
        .expect("P-256 uncompressed points are exactly 65 bytes");
    let mac_ephemeral_public_key =
        PublicKeyBytes::try_new(public_bytes).expect("P-256 generated a valid public key");

    let mut mac_nonce = [0_u8; 32];
    rng.fill_bytes(&mut mac_nonce)
        .map_err(|_| IssueError::Randomness)?;
    let message = Challenge {
        mac_id: parameters.mac_id,
        device_id: parameters.device_id,
        pairing_generation: parameters.pairing_generation,
        binding: request.binding(),
        challenge_id: request.challenge_id(),
        counter_floor: parameters.counter_floor,
        ttl_ms,
        mac_nonce,
        mac_ephemeral_public_key,
        mac_identity_signature: [0; 64],
    };
    let mut frame = wire::encode_challenge(&message);
    let prehash = transcript::mac_challenge_signature_hash(&frame);
    let signing_request = MacChallengeSigningRequest { prehash: &prehash };
    let raw_signature = signer
        .sign_challenge(&signing_request)
        .map_err(|_| IssueError::Signing)?;
    let signature =
        Signature::from_slice(&raw_signature).map_err(|_| IssueError::InvalidSignatureEncoding)?;
    if signature.normalize_s().is_some() {
        return Err(IssueError::HighS);
    }
    let verifying_key =
        VerifyingKey::from_sec1_bytes(parameters.mac_identity_public_key.as_bytes())
            .map_err(|_| IssueError::Signature)?;
    verifying_key
        .verify_prehash(&prehash, &signature)
        .map_err(|_| IssueError::Signature)?;
    let mut message = message;
    message.mac_identity_signature = raw_signature;
    frame[wire::CHALLENGE_SIGNATURE_OFFSET..].copy_from_slice(&raw_signature);
    Ok(IssuedChallenge {
        message,
        frame,
        issued_at: parameters.issued_at,
        deadline: request.deadline(),
        mac_ephemeral_private_key: private_bytes,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeVerificationError {
    Wire(wire::WireError),
    PairingMismatch,
    InvalidSignatureEncoding,
    HighS,
    Signature,
}

impl Display for ChallengeVerificationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "v1 Mac challenge rejected: {self:?}")
    }
}

impl Error for ChallengeVerificationError {}

/// Opaque phone-side evidence that a Challenge came from the paired Mac.
///
/// This value is intentionally neither `Clone` nor `Debug`, and it is the only
/// accepted input to `build_phone_response`.
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::VerifiedMacChallenge;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<VerifiedMacChallenge>();
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::VerifiedMacChallenge;
/// fn assert_debug<T: std::fmt::Debug>() {}
/// assert_debug::<VerifiedMacChallenge>();
/// ```
pub struct VerifiedMacChallenge {
    message: Challenge,
    frame: [u8; wire::CHALLENGE_FRAME_LEN],
    phone_identity_public_key: PublicKeyBytes,
}

impl VerifiedMacChallenge {
    #[must_use]
    pub(crate) const fn message(&self) -> &Challenge {
        &self.message
    }

    #[must_use]
    pub(crate) const fn frame(&self) -> &[u8; wire::CHALLENGE_FRAME_LEN] {
        &self.frame
    }

    #[must_use]
    pub(crate) const fn phone_identity_public_key(&self) -> PublicKeyBytes {
        self.phone_identity_public_key
    }

    #[must_use]
    pub const fn mac_id(&self) -> MacId {
        self.message.mac_id
    }

    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        self.message.device_id
    }

    #[must_use]
    pub const fn pairing_generation(&self) -> PairingGeneration {
        self.message.pairing_generation
    }

    #[must_use]
    pub const fn binding(&self) -> SessionBinding {
        self.message.binding
    }

    #[must_use]
    pub const fn challenge_id(&self) -> ChallengeId {
        self.message.challenge_id
    }
}

pub fn verify_mac_challenge(
    challenge_frame: &[u8],
    paired_mac: &PairedMac,
) -> Result<VerifiedMacChallenge, ChallengeVerificationError> {
    let message =
        wire::decode_challenge(challenge_frame).map_err(ChallengeVerificationError::Wire)?;
    if message.mac_id != paired_mac.mac_id
        || message.device_id != paired_mac.device_id
        || message.pairing_generation != paired_mac.pairing_generation
    {
        return Err(ChallengeVerificationError::PairingMismatch);
    }
    let frame: [u8; wire::CHALLENGE_FRAME_LEN] = challenge_frame
        .try_into()
        .map_err(|_| ChallengeVerificationError::Wire(wire::WireError::LengthMismatch))?;
    let signature = Signature::from_slice(message.mac_identity_signature())
        .map_err(|_| ChallengeVerificationError::InvalidSignatureEncoding)?;
    if signature.normalize_s().is_some() {
        return Err(ChallengeVerificationError::HighS);
    }
    let verifying_key =
        VerifyingKey::from_sec1_bytes(paired_mac.mac_identity_public_key.as_bytes())
            .map_err(|_| ChallengeVerificationError::Signature)?;
    verifying_key
        .verify_prehash(
            &transcript::mac_challenge_signature_hash(&frame),
            &signature,
        )
        .map_err(|_| ChallengeVerificationError::Signature)?;
    Ok(VerifiedMacChallenge {
        message,
        frame,
        phone_identity_public_key: paired_mac.phone_identity_public_key,
    })
}

/// Direction-separated session material. Secret bytes are zeroized on drop and
/// the container is intentionally neither `Clone` nor `Debug`.
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::SessionMaterial;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<SessionMaterial>();
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::SessionMaterial;
/// fn assert_debug<T: std::fmt::Debug>() {}
/// assert_debug::<SessionMaterial>();
/// ```
#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct SessionMaterial {
    mac_to_phone_key: [u8; 32],
    mac_to_phone_nonce: [u8; 12],
    phone_to_mac_key: [u8; 32],
    phone_to_mac_nonce: [u8; 12],
}

impl SessionMaterial {
    #[must_use]
    pub(crate) const fn mac_to_phone_key(&self) -> &[u8; 32] {
        &self.mac_to_phone_key
    }

    #[must_use]
    pub(crate) const fn mac_to_phone_nonce(&self) -> &[u8; 12] {
        &self.mac_to_phone_nonce
    }

    #[must_use]
    pub(crate) const fn phone_to_mac_key(&self) -> &[u8; 32] {
        &self.phone_to_mac_key
    }

    #[must_use]
    pub(crate) const fn phone_to_mac_nonce(&self) -> &[u8; 12] {
        &self.phone_to_mac_nonce
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    InvalidMacPrivateKey,
    InvalidPhonePublicKey,
    InvalidPhonePrivateKey,
    InvalidMacPublicKey,
    Hkdf,
    Aead,
    ProofMismatch,
}

impl Display for CryptoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "v1 cryptographic operation failed: {self:?}")
    }
}

impl Error for CryptoError {}

fn mac_shared_secret(
    issued: &IssuedChallenge,
    response: &Response,
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let private_key = SecretKey::from_slice(issued.mac_ephemeral_private_key.as_ref())
        .map_err(|_| CryptoError::InvalidMacPrivateKey)?;
    let phone_public = response
        .phone_ephemeral_public_key
        .to_public_key()
        .map_err(|_| CryptoError::InvalidPhonePublicKey)?;
    let private_scalar = Zeroizing::new(private_key.to_nonzero_scalar());
    let shared = diffie_hellman(&*private_scalar, phone_public.as_affine());
    let mut shared_secret = Zeroizing::new([0_u8; 32]);
    shared_secret.copy_from_slice(shared.raw_secret_bytes().as_slice());
    Ok(shared_secret)
}

fn phone_shared_secret(
    private_key: &SecretKey,
    mac_public_key: PublicKeyBytes,
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let mac_public = mac_public_key
        .to_public_key()
        .map_err(|_| CryptoError::InvalidMacPublicKey)?;
    let private_scalar = Zeroizing::new(private_key.to_nonzero_scalar());
    let shared = diffie_hellman(&*private_scalar, mac_public.as_affine());
    let mut shared_secret = Zeroizing::new([0_u8; 32]);
    shared_secret.copy_from_slice(shared.raw_secret_bytes().as_slice());
    Ok(shared_secret)
}

fn derive_directional_material(
    shared_secret: &[u8; 32],
    challenge_frame: &[u8; wire::CHALLENGE_FRAME_LEN],
    response: &Response,
) -> Result<SessionMaterial, CryptoError> {
    let context = transcript::context_hash(challenge_frame, response);
    let salt = transcript::salt(&context);
    let mut material = SessionMaterial {
        mac_to_phone_key: [0; 32],
        mac_to_phone_nonce: [0; 12],
        phone_to_mac_key: [0; 32],
        phone_to_mac_nonce: [0; 12],
    };
    {
        // hkdf 0.12 does not expose a ZeroizeOnDrop PRK. Keep this opaque
        // container in the narrowest scope; all input/output byte buffers are
        // separately zeroizing.
        let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared_secret);
        hkdf.expand(MAC_TO_PHONE_KEY_LABEL, &mut material.mac_to_phone_key)
            .map_err(|_| CryptoError::Hkdf)?;
        hkdf.expand(MAC_TO_PHONE_NONCE_LABEL, &mut material.mac_to_phone_nonce)
            .map_err(|_| CryptoError::Hkdf)?;
        hkdf.expand(PHONE_TO_MAC_KEY_LABEL, &mut material.phone_to_mac_key)
            .map_err(|_| CryptoError::Hkdf)?;
        hkdf.expand(PHONE_TO_MAC_NONCE_LABEL, &mut material.phone_to_mac_nonce)
            .map_err(|_| CryptoError::Hkdf)?;
    }
    Ok(material)
}

pub(crate) fn derive_session_material(
    issued: &IssuedChallenge,
    response: &Response,
) -> Result<SessionMaterial, CryptoError> {
    let shared_secret = mac_shared_secret(issued, response)?;
    derive_directional_material(&shared_secret, &issued.frame, response)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneBuildError {
    CounterOverflow,
    Randomness,
    InvalidPrivateKeyEntropy,
    Signing,
    InvalidSignatureEncoding,
    HighS,
    Signature,
    Crypto(CryptoError),
}

impl Display for PhoneBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot build v1 phone response: {self:?}")
    }
}

impl Error for PhoneBuildError {}

/// Build exactly one response after authenticating the paired Mac challenge.
///
/// `previous_counter` is the phone's last durable counter. The builder advances
/// `max(previous_counter, challenge.counter_floor)` by exactly one and rejects
/// overflow before invoking randomness or the identity signer.
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::{build_phone_response, CryptoRandom, PhoneResponseSigner};
/// use repose_unlock_core::protocol::messages::Challenge;
/// fn cannot_build<R: CryptoRandom, S: PhoneResponseSigner>(
///     raw: Challenge,
///     rng: &mut R,
///     signer: &mut S,
/// ) {
///     let _ = build_phone_response(raw, 0, rng, signer);
/// }
/// ```
pub(crate) fn build_phone_response<R: CryptoRandom, S: PhoneResponseSigner>(
    authenticated_challenge: VerifiedMacChallenge,
    previous_counter: u64,
    rng: &mut R,
    signer: &mut S,
) -> Result<[u8; wire::RESPONSE_FRAME_LEN], PhoneBuildError> {
    let counter = previous_counter
        .max(authenticated_challenge.message.counter_floor)
        .checked_add(1)
        .ok_or(PhoneBuildError::CounterOverflow)?;

    let mut private_bytes = Zeroizing::new([0_u8; 32]);
    let mut private_key = None;
    for _ in 0..PRIVATE_KEY_ATTEMPTS {
        rng.fill_bytes(private_bytes.as_mut())
            .map_err(|_| PhoneBuildError::Randomness)?;
        if let Ok(candidate) = SecretKey::from_slice(private_bytes.as_ref()) {
            private_key = Some(candidate);
            break;
        }
    }
    let private_key = private_key.ok_or(PhoneBuildError::InvalidPrivateKeyEntropy)?;
    let encoded_public = private_key.public_key().to_encoded_point(false);
    let public_bytes: [u8; 65] = encoded_public
        .as_bytes()
        .try_into()
        .expect("P-256 uncompressed points are exactly 65 bytes");
    let phone_ephemeral_public_key =
        PublicKeyBytes::try_new(public_bytes).expect("P-256 generated a valid public key");

    let mut phone_nonce = [0_u8; 32];
    rng.fill_bytes(&mut phone_nonce)
        .map_err(|_| PhoneBuildError::Randomness)?;
    let challenge = &authenticated_challenge.message;
    let mut response = Response {
        mac_id: challenge.mac_id,
        device_id: challenge.device_id,
        pairing_generation: challenge.pairing_generation,
        binding: challenge.binding,
        challenge_id: challenge.challenge_id,
        counter,
        mac_nonce: challenge.mac_nonce,
        phone_nonce,
        mac_ephemeral_public_key: challenge.mac_ephemeral_public_key,
        phone_ephemeral_public_key,
        ciphertext: [0; 32],
        tag: [0; 16],
        signature: [0; 64],
    };

    let shared_secret = phone_shared_secret(&private_key, challenge.mac_ephemeral_public_key)
        .map_err(PhoneBuildError::Crypto)?;
    let material =
        derive_directional_material(&shared_secret, &authenticated_challenge.frame, &response)
            .map_err(PhoneBuildError::Crypto)?;
    let context_hash = transcript::context_hash(&authenticated_challenge.frame, &response);
    let mut plaintext = Zeroizing::new(transcript::proof(&context_hash));
    let cipher = Aes256Gcm::new_from_slice(material.phone_to_mac_key())
        .map_err(|_| PhoneBuildError::Crypto(CryptoError::Aead))?;
    let aad = transcript::aad(&authenticated_challenge.frame, &response);
    let tag = cipher
        .encrypt_in_place_detached(
            Nonce::from_slice(material.phone_to_mac_nonce()),
            &aad,
            plaintext.as_mut(),
        )
        .map_err(|_| PhoneBuildError::Crypto(CryptoError::Aead))?;
    response.ciphertext.copy_from_slice(plaintext.as_ref());
    response.tag.copy_from_slice(tag.as_slice());

    drop(cipher);
    drop(plaintext);
    drop(material);
    drop(shared_secret);
    drop(private_key);
    drop(private_bytes);

    let prehash = transcript::signature_hash(&authenticated_challenge.frame, &response);
    let request = PhoneResponseSigningRequest { prehash: &prehash };
    let raw_signature = signer
        .sign_response(&request)
        .map_err(|_| PhoneBuildError::Signing)?;
    response.signature = raw_signature;
    verify_phone_response_signature(
        &authenticated_challenge.frame,
        &response,
        authenticated_challenge.phone_identity_public_key,
    )?;
    Ok(wire::encode_response(&response))
}

pub(crate) fn verify_phone_response_signature(
    challenge_frame: &[u8; wire::CHALLENGE_FRAME_LEN],
    response: &Response,
    phone_identity_public_key: PublicKeyBytes,
) -> Result<(), PhoneBuildError> {
    let signature = Signature::from_slice(response.signature())
        .map_err(|_| PhoneBuildError::InvalidSignatureEncoding)?;
    if signature.normalize_s().is_some() {
        return Err(PhoneBuildError::HighS);
    }
    let verifying_key = VerifyingKey::from_sec1_bytes(phone_identity_public_key.as_bytes())
        .map_err(|_| PhoneBuildError::Signature)?;
    verifying_key
        .verify_prehash(
            &transcript::signature_hash(challenge_frame, response),
            &signature,
        )
        .map_err(|_| PhoneBuildError::Signature)
}

fn decrypt_response_proof(
    issued: &IssuedChallenge,
    response: &Response,
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let material = derive_session_material(issued, response)?;
    let cipher =
        Aes256Gcm::new_from_slice(material.phone_to_mac_key()).map_err(|_| CryptoError::Aead)?;
    let mut plaintext = Zeroizing::new(*response.ciphertext());
    let tag = Tag::from_slice(response.tag());
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(material.phone_to_mac_nonce()),
            &transcript::aad(&issued.frame, response),
            plaintext.as_mut(),
            tag,
        )
        .map_err(|_| CryptoError::Aead)?;
    let expected = Zeroizing::new(transcript::proof(&transcript::context_hash(
        &issued.frame,
        response,
    )));
    if plaintext.as_ref().ct_eq(expected.as_ref()).unwrap_u8() != 1 {
        return Err(CryptoError::ProofMismatch);
    }
    Ok(plaintext)
}

/// Expected bytes for the committed interoperability fixture.
///
/// This diagnostic type never returns derived secret material to its caller.
#[doc(hidden)]
pub struct TestVectorExpectations<'a> {
    pub mac_challenge_signature_hash: &'a [u8; 32],
    pub shared_secret: &'a [u8; 32],
    pub context_hash: &'a [u8; 32],
    pub hkdf_salt: &'a [u8; 32],
    pub mac_to_phone_key: &'a [u8; 32],
    pub mac_to_phone_nonce: &'a [u8; 12],
    pub phone_to_mac_key: &'a [u8; 32],
    pub phone_to_mac_nonce: &'a [u8; 12],
    pub response_aad: &'a [u8],
    pub response_plaintext: &'a [u8; 32],
    pub response_signature_hash: &'a [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum TestVectorError {
    Wire(wire::WireError),
    Crypto(CryptoError),
    Mismatch,
}

/// Compare the internal v1 derivation with committed expected bytes without
/// exposing the computed shared secret or session keys.
#[doc(hidden)]
pub fn verify_test_vector(
    issued: &IssuedChallenge,
    response_frame: &[u8],
    expected: &TestVectorExpectations<'_>,
) -> Result<(), TestVectorError> {
    let response = wire::decode_response(response_frame).map_err(TestVectorError::Wire)?;
    let shared_secret = mac_shared_secret(issued, &response).map_err(TestVectorError::Crypto)?;
    let material = derive_directional_material(&shared_secret, &issued.frame, &response)
        .map_err(TestVectorError::Crypto)?;
    let context_hash = transcript::context_hash(&issued.frame, &response);
    let salt = transcript::salt(&context_hash);
    let aad = transcript::aad(&issued.frame, &response);
    let plaintext = Zeroizing::new(transcript::proof(&context_hash));
    let decrypted = decrypt_response_proof(issued, &response).map_err(TestVectorError::Crypto)?;
    let signature_hash = transcript::signature_hash(&issued.frame, &response);
    let challenge_signature_hash = transcript::mac_challenge_signature_hash(&issued.frame);

    let matches = shared_secret.ct_eq(expected.shared_secret).unwrap_u8() == 1
        && material
            .mac_to_phone_key()
            .ct_eq(expected.mac_to_phone_key)
            .unwrap_u8()
            == 1
        && material
            .mac_to_phone_nonce()
            .ct_eq(expected.mac_to_phone_nonce)
            .unwrap_u8()
            == 1
        && material
            .phone_to_mac_key()
            .ct_eq(expected.phone_to_mac_key)
            .unwrap_u8()
            == 1
        && material
            .phone_to_mac_nonce()
            .ct_eq(expected.phone_to_mac_nonce)
            .unwrap_u8()
            == 1
        && plaintext.ct_eq(expected.response_plaintext).unwrap_u8() == 1
        && decrypted.ct_eq(expected.response_plaintext).unwrap_u8() == 1
        && context_hash == *expected.context_hash
        && salt == *expected.hkdf_salt
        && aad == expected.response_aad
        && signature_hash == *expected.response_signature_hash
        && challenge_signature_hash == *expected.mac_challenge_signature_hash;
    if matches {
        Ok(())
    } else {
        Err(TestVectorError::Mismatch)
    }
}

/// An opaque proof that wire, local context, signature, ECDH, and AEAD checks passed.
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::AuthenticatedResponse;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<AuthenticatedResponse>();
/// ```
///
/// ```compile_fail
/// use repose_unlock_core::protocol::crypto::AuthenticatedResponse;
/// fn assert_copy<T: Copy>() {}
/// assert_copy::<AuthenticatedResponse>();
/// ```
pub struct AuthenticatedResponse {
    pub(crate) mac_id: MacId,
    pub(crate) device_id: DeviceId,
    pub(crate) pairing_generation: PairingGeneration,
    pub(crate) binding: SessionBinding,
    pub(crate) challenge_id: ChallengeId,
    pub(crate) counter: u64,
}

impl AuthenticatedResponse {
    #[must_use]
    pub const fn mac_id(&self) -> MacId {
        self.mac_id
    }

    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn pairing_generation(&self) -> PairingGeneration {
        self.pairing_generation
    }

    #[must_use]
    pub const fn binding(&self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn challenge_id(&self) -> ChallengeId {
        self.challenge_id
    }

    #[must_use]
    pub const fn counter(&self) -> u64 {
        self.counter
    }
}

impl Debug for AuthenticatedResponse {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthenticatedResponse(<opaque>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationError {
    Wire(wire::WireError),
    NonMonotonicTime,
    Expired,
    PairingMismatch,
    SessionMismatch,
    ChallengeMismatch,
    CounterNotAdvanced,
    InvalidSignatureEncoding,
    HighS,
    Signature,
    Crypto(CryptoError),
}

impl Display for VerificationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "v1 response rejected: {self:?}")
    }
}

impl Error for VerificationError {}

pub fn verify_response(
    issued: &IssuedChallenge,
    response_frame: &[u8],
    context: &VerificationContext,
    now: MonoMillis,
) -> Result<AuthenticatedResponse, VerificationError> {
    if now < issued.issued_at {
        return Err(VerificationError::NonMonotonicTime);
    }
    if now >= issued.deadline {
        return Err(VerificationError::Expired);
    }
    let response = wire::decode_response(response_frame).map_err(VerificationError::Wire)?;
    let challenge = &issued.message;
    let paired = context.paired_device;
    if paired.mac_id != challenge.mac_id
        || paired.device_id != challenge.device_id
        || paired.pairing_generation != challenge.pairing_generation
        || response.mac_id != paired.mac_id
        || response.device_id != paired.device_id
        || response.pairing_generation != paired.pairing_generation
    {
        return Err(VerificationError::PairingMismatch);
    }
    if context.authoritative_binding != challenge.binding
        || response.binding != context.authoritative_binding
    {
        return Err(VerificationError::SessionMismatch);
    }
    if response.challenge_id != challenge.challenge_id
        || response.mac_nonce != challenge.mac_nonce
        || response.mac_ephemeral_public_key != challenge.mac_ephemeral_public_key
    {
        return Err(VerificationError::ChallengeMismatch);
    }
    if response.counter <= challenge.counter_floor || response.counter == 0 {
        return Err(VerificationError::CounterNotAdvanced);
    }

    let signature = Signature::from_slice(response.signature())
        .map_err(|_| VerificationError::InvalidSignatureEncoding)?;
    if signature.normalize_s().is_some() {
        return Err(VerificationError::HighS);
    }
    let verifying_key = VerifyingKey::from_sec1_bytes(paired.phone_identity_public_key.as_bytes())
        .map_err(|_| VerificationError::Signature)?;
    verifying_key
        .verify_prehash(
            &transcript::signature_hash(&issued.frame, &response),
            &signature,
        )
        .map_err(|_| VerificationError::Signature)?;
    decrypt_response_proof(issued, &response).map_err(VerificationError::Crypto)?;

    Ok(AuthenticatedResponse {
        mac_id: response.mac_id,
        device_id: response.device_id,
        pairing_generation: response.pairing_generation,
        binding: response.binding,
        challenge_id: response.challenge_id,
        counter: response.counter,
    })
}
