use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::domain::{AuditSessionId, ConsoleUid, LockEpoch};
use crate::protocol::messages::{
    CIPHERTEXT_LEN, Challenge, DeviceId, ID_LEN, MacId, NONCE_LEN, PUBLIC_KEY_LEN,
    PairingGeneration, PublicKeyBytes, Response, SIGNATURE_LEN, TAG_LEN,
};
use crate::state_machine::{ChallengeId, SessionBinding};

pub const MAGIC: [u8; 4] = *b"RPUK";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 12;
pub const CHALLENGE_PAYLOAD_LEN: usize = 237;
pub const CHALLENGE_FRAME_LEN: usize = HEADER_LEN + CHALLENGE_PAYLOAD_LEN;
pub const RESPONSE_PAYLOAD_LEN: usize = 378;
pub const RESPONSE_FRAME_LEN: usize = HEADER_LEN + RESPONSE_PAYLOAD_LEN;
pub const MAX_PAYLOAD_LEN: usize = RESPONSE_PAYLOAD_LEN;
pub const MAX_FRAME_LEN: usize = RESPONSE_FRAME_LEN;

pub const CHALLENGE_MAC_PUBLIC_KEY_OFFSET: usize = 120;
pub const CHALLENGE_SIGNATURE_OFFSET: usize = 185;
pub const CHALLENGE_SIGNED_PREFIX_LEN: usize = CHALLENGE_SIGNATURE_OFFSET;
pub const RESPONSE_MAC_PUBLIC_KEY_OFFSET: usize = 148;
pub const RESPONSE_PHONE_PUBLIC_KEY_OFFSET: usize = 213;
pub const RESPONSE_CIPHERTEXT_OFFSET: usize = 278;
pub const RESPONSE_TAG_OFFSET: usize = RESPONSE_CIPHERTEXT_OFFSET + CIPHERTEXT_LEN;
pub const RESPONSE_SIGNATURE_OFFSET: usize = RESPONSE_TAG_OFFSET + TAG_LEN;
pub const RESPONSE_PREFIX_LEN: usize = RESPONSE_CIPHERTEXT_OFFSET;
pub const RESPONSE_SIGNED_PREFIX_LEN: usize = RESPONSE_SIGNATURE_OFFSET;

const KIND_CHALLENGE: u8 = 1;
const KIND_RESPONSE: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    Truncated,
    TrailingBytes,
    BadMagic,
    UnknownVersion,
    UnknownKind,
    UnexpectedKind,
    NonZeroFlags,
    PayloadTooLarge,
    LengthMismatch,
    InvalidTtl,
    InvalidPublicKey,
}

impl Display for WireError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid v1 protocol frame: {self:?}")
    }
}

impl Error for WireError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Challenge,
    Response,
}

impl Kind {
    const fn byte(self) -> u8 {
        match self {
            Self::Challenge => KIND_CHALLENGE,
            Self::Response => KIND_RESPONSE,
        }
    }

    const fn payload_len(self) -> usize {
        match self {
            Self::Challenge => CHALLENGE_PAYLOAD_LEN,
            Self::Response => RESPONSE_PAYLOAD_LEN,
        }
    }
}

pub fn encode_challenge(challenge: &Challenge) -> [u8; CHALLENGE_FRAME_LEN] {
    let mut frame = [0_u8; CHALLENGE_FRAME_LEN];
    encode_header(&mut frame[..HEADER_LEN], Kind::Challenge);
    frame[12..28].copy_from_slice(challenge.mac_id.as_bytes());
    frame[28..44].copy_from_slice(challenge.device_id.as_bytes());
    frame[44..52].copy_from_slice(&challenge.pairing_generation.get().to_be_bytes());
    frame[52..56].copy_from_slice(&challenge.binding.console_uid().get().to_be_bytes());
    frame[56..60].copy_from_slice(&challenge.binding.audit_session_id().get().to_be_bytes());
    frame[60..68].copy_from_slice(&challenge.binding.lock_epoch().get().to_be_bytes());
    frame[68..76].copy_from_slice(&challenge.challenge_id.get().to_be_bytes());
    frame[76..84].copy_from_slice(&challenge.counter_floor.to_be_bytes());
    frame[84..88].copy_from_slice(&challenge.ttl_ms.to_be_bytes());
    frame[88..120].copy_from_slice(&challenge.mac_nonce);
    frame[120..185].copy_from_slice(challenge.mac_ephemeral_public_key.as_bytes());
    frame[CHALLENGE_SIGNATURE_OFFSET..].copy_from_slice(&challenge.mac_identity_signature);
    frame
}

pub fn decode_challenge(frame: &[u8]) -> Result<Challenge, WireError> {
    parse_header(frame, Kind::Challenge)?;
    let ttl_ms = read_u32(frame, 84);
    if ttl_ms == 0 || u64::from(ttl_ms) > crate::state_machine::TimingPolicy::MAX_CHALLENGE_TTL_MS {
        return Err(WireError::InvalidTtl);
    }
    let mac_public =
        PublicKeyBytes::try_new(read_array(frame, 120)).map_err(|_| WireError::InvalidPublicKey)?;
    Ok(Challenge {
        mac_id: MacId::new(read_array(frame, 12)),
        device_id: DeviceId::new(read_array(frame, 28)),
        pairing_generation: PairingGeneration::new(read_u64(frame, 44)),
        binding: SessionBinding::new(
            LockEpoch::new(read_u64(frame, 60)),
            AuditSessionId::new(read_u32(frame, 56)),
            ConsoleUid::new(read_u32(frame, 52)),
        ),
        challenge_id: ChallengeId::from_protocol(read_u64(frame, 68)),
        counter_floor: read_u64(frame, 76),
        ttl_ms,
        mac_nonce: read_array(frame, 88),
        mac_ephemeral_public_key: mac_public,
        mac_identity_signature: read_array(frame, CHALLENGE_SIGNATURE_OFFSET),
    })
}

#[must_use]
pub fn encode_response(response: &Response) -> [u8; RESPONSE_FRAME_LEN] {
    let mut frame = [0_u8; RESPONSE_FRAME_LEN];
    encode_header(&mut frame[..HEADER_LEN], Kind::Response);
    frame[12..28].copy_from_slice(response.mac_id.as_bytes());
    frame[28..44].copy_from_slice(response.device_id.as_bytes());
    frame[44..52].copy_from_slice(&response.pairing_generation.get().to_be_bytes());
    frame[52..56].copy_from_slice(&response.binding.console_uid().get().to_be_bytes());
    frame[56..60].copy_from_slice(&response.binding.audit_session_id().get().to_be_bytes());
    frame[60..68].copy_from_slice(&response.binding.lock_epoch().get().to_be_bytes());
    frame[68..76].copy_from_slice(&response.challenge_id.get().to_be_bytes());
    frame[76..84].copy_from_slice(&response.counter.to_be_bytes());
    frame[84..116].copy_from_slice(&response.mac_nonce);
    frame[116..148].copy_from_slice(&response.phone_nonce);
    frame[148..213].copy_from_slice(response.mac_ephemeral_public_key.as_bytes());
    frame[213..278].copy_from_slice(response.phone_ephemeral_public_key.as_bytes());
    frame[278..310].copy_from_slice(&response.ciphertext);
    frame[310..326].copy_from_slice(&response.tag);
    frame[326..390].copy_from_slice(&response.signature);
    frame
}

pub fn decode_response(frame: &[u8]) -> Result<Response, WireError> {
    parse_header(frame, Kind::Response)?;
    let mac_public = PublicKeyBytes::try_new(read_array(frame, RESPONSE_MAC_PUBLIC_KEY_OFFSET))
        .map_err(|_| WireError::InvalidPublicKey)?;
    let phone_public = PublicKeyBytes::try_new(read_array(frame, RESPONSE_PHONE_PUBLIC_KEY_OFFSET))
        .map_err(|_| WireError::InvalidPublicKey)?;
    Ok(Response {
        mac_id: MacId::new(read_array(frame, 12)),
        device_id: DeviceId::new(read_array(frame, 28)),
        pairing_generation: PairingGeneration::new(read_u64(frame, 44)),
        binding: SessionBinding::new(
            LockEpoch::new(read_u64(frame, 60)),
            AuditSessionId::new(read_u32(frame, 56)),
            ConsoleUid::new(read_u32(frame, 52)),
        ),
        challenge_id: ChallengeId::from_protocol(read_u64(frame, 68)),
        counter: read_u64(frame, 76),
        mac_nonce: read_array(frame, 84),
        phone_nonce: read_array(frame, 116),
        mac_ephemeral_public_key: mac_public,
        phone_ephemeral_public_key: phone_public,
        ciphertext: read_array(frame, RESPONSE_CIPHERTEXT_OFFSET),
        tag: read_array(frame, RESPONSE_TAG_OFFSET),
        signature: read_array(frame, RESPONSE_SIGNATURE_OFFSET),
    })
}

fn encode_header(output: &mut [u8], kind: Kind) {
    output[0..4].copy_from_slice(&MAGIC);
    output[4] = VERSION;
    output[5] = kind.byte();
    output[6..8].fill(0);
    output[8..12].copy_from_slice(&(kind.payload_len() as u32).to_be_bytes());
}

fn parse_header(frame: &[u8], expected: Kind) -> Result<(), WireError> {
    if frame.len() < HEADER_LEN {
        return Err(WireError::Truncated);
    }
    if frame[0..4] != MAGIC {
        return Err(WireError::BadMagic);
    }
    if frame[4] != VERSION {
        return Err(WireError::UnknownVersion);
    }
    let actual = match frame[5] {
        KIND_CHALLENGE => Kind::Challenge,
        KIND_RESPONSE => Kind::Response,
        _ => return Err(WireError::UnknownKind),
    };
    if actual != expected {
        return Err(WireError::UnexpectedKind);
    }
    if frame[6] != 0 || frame[7] != 0 {
        return Err(WireError::NonZeroFlags);
    }
    let declared = usize::try_from(read_u32(frame, 8)).map_err(|_| WireError::PayloadTooLarge)?;
    if declared > MAX_PAYLOAD_LEN {
        return Err(WireError::PayloadTooLarge);
    }
    if declared != expected.payload_len() {
        return Err(WireError::LengthMismatch);
    }
    let total = HEADER_LEN
        .checked_add(declared)
        .ok_or(WireError::PayloadTooLarge)?;
    match frame.len().cmp(&total) {
        std::cmp::Ordering::Less => Err(WireError::Truncated),
        std::cmp::Ordering::Greater => Err(WireError::TrailingBytes),
        std::cmp::Ordering::Equal => Ok(()),
    }
}

fn read_u32(frame: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(read_array(frame, offset))
}

fn read_u64(frame: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(read_array(frame, offset))
}

fn read_array<const N: usize>(frame: &[u8], offset: usize) -> [u8; N] {
    // Every caller runs only after exact-length validation and uses compile-time fixed offsets.
    frame[offset..offset + N]
        .try_into()
        .expect("validated fixed protocol offset")
}

const _: () = {
    assert!(ID_LEN == 16);
    assert!(NONCE_LEN == 32);
    assert!(PUBLIC_KEY_LEN == 65);
    assert!(SIGNATURE_LEN == 64);
    assert!(CHALLENGE_SIGNATURE_OFFSET + SIGNATURE_LEN == CHALLENGE_FRAME_LEN);
    assert!(RESPONSE_SIGNATURE_OFFSET + SIGNATURE_LEN == RESPONSE_FRAME_LEN);
};
