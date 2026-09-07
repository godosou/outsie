use std::error::Error;
use std::fmt::{self, Display, Formatter};

use p256::PublicKey;

use crate::state_machine::{ChallengeId, SessionBinding};

pub const ID_LEN: usize = 16;
pub const NONCE_LEN: usize = 32;
pub const PUBLIC_KEY_LEN: usize = 65;
pub const SIGNATURE_LEN: usize = 64;
pub const CIPHERTEXT_LEN: usize = 32;
pub const TAG_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacId([u8; ID_LEN]);

impl MacId {
    #[must_use]
    pub const fn new(bytes: [u8; ID_LEN]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; ID_LEN] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId([u8; ID_LEN]);

impl DeviceId {
    #[must_use]
    pub const fn new(bytes: [u8; ID_LEN]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; ID_LEN] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PairingGeneration(u64);

impl PairingGeneration {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKeyBytes([u8; PUBLIC_KEY_LEN]);

impl PublicKeyBytes {
    pub fn try_new(bytes: [u8; PUBLIC_KEY_LEN]) -> Result<Self, PublicKeyError> {
        if bytes[0] != 0x04 || PublicKey::from_sec1_bytes(&bytes).is_err() {
            return Err(PublicKeyError);
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; PUBLIC_KEY_LEN] {
        &self.0
    }

    pub(crate) fn to_public_key(self) -> Result<PublicKey, PublicKeyError> {
        PublicKey::from_sec1_bytes(&self.0).map_err(|_| PublicKeyError)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKeyError;

impl Display for PublicKeyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid uncompressed P-256 public key")
    }
}

impl Error for PublicKeyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Challenge {
    pub(crate) mac_id: MacId,
    pub(crate) device_id: DeviceId,
    pub(crate) pairing_generation: PairingGeneration,
    pub(crate) binding: SessionBinding,
    pub(crate) challenge_id: ChallengeId,
    pub(crate) counter_floor: u64,
    pub(crate) ttl_ms: u32,
    pub(crate) mac_nonce: [u8; NONCE_LEN],
    pub(crate) mac_ephemeral_public_key: PublicKeyBytes,
}

impl Challenge {
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
    pub const fn counter_floor(&self) -> u64 {
        self.counter_floor
    }

    #[must_use]
    pub const fn ttl_ms(&self) -> u32 {
        self.ttl_ms
    }

    #[must_use]
    pub const fn mac_nonce(&self) -> &[u8; NONCE_LEN] {
        &self.mac_nonce
    }

    #[must_use]
    pub const fn mac_ephemeral_public_key(&self) -> PublicKeyBytes {
        self.mac_ephemeral_public_key
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response {
    pub(crate) mac_id: MacId,
    pub(crate) device_id: DeviceId,
    pub(crate) pairing_generation: PairingGeneration,
    pub(crate) binding: SessionBinding,
    pub(crate) challenge_id: ChallengeId,
    pub(crate) counter: u64,
    pub(crate) mac_nonce: [u8; NONCE_LEN],
    pub(crate) phone_nonce: [u8; NONCE_LEN],
    pub(crate) mac_ephemeral_public_key: PublicKeyBytes,
    pub(crate) phone_ephemeral_public_key: PublicKeyBytes,
    pub(crate) ciphertext: [u8; CIPHERTEXT_LEN],
    pub(crate) tag: [u8; TAG_LEN],
    pub(crate) signature: [u8; SIGNATURE_LEN],
}

impl Response {
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

    #[must_use]
    pub const fn mac_nonce(&self) -> &[u8; NONCE_LEN] {
        &self.mac_nonce
    }

    #[must_use]
    pub const fn phone_nonce(&self) -> &[u8; NONCE_LEN] {
        &self.phone_nonce
    }

    #[must_use]
    pub const fn mac_ephemeral_public_key(&self) -> PublicKeyBytes {
        self.mac_ephemeral_public_key
    }

    #[must_use]
    pub const fn phone_ephemeral_public_key(&self) -> PublicKeyBytes {
        self.phone_ephemeral_public_key
    }

    #[must_use]
    pub const fn ciphertext(&self) -> &[u8; CIPHERTEXT_LEN] {
        &self.ciphertext
    }

    #[must_use]
    pub const fn tag(&self) -> &[u8; TAG_LEN] {
        &self.tag
    }

    #[must_use]
    pub const fn signature(&self) -> &[u8; SIGNATURE_LEN] {
        &self.signature
    }
}
