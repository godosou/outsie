use std::error::Error;
use std::fmt::{self, Display, Formatter};

use p256::PublicKey;
use zeroize::Zeroizing;

pub const BLE_SERVICE_UUID: &str = "A53E0001-7A6B-4D59-9F2E-5245504F5345";
pub const BLE_CONTROL_CHARACTERISTIC_UUID: &str = "A53E0002-7A6B-4D59-9F2E-5245504F5345";
pub const BLE_STATUS_CHARACTERISTIC_UUID: &str = "A53E0003-7A6B-4D59-9F2E-5245504F5345";

pub const PAIRING_HEADER_LEN: usize = 8;
pub const PAIRING_SESSION_ID_LEN: usize = 16;
pub const PAIRING_MAC_ID_LEN: usize = 16;
pub const PAIRING_PUBLIC_KEY_LEN: usize = 65;
pub const PAIRING_SECRET_LEN: usize = 32;
pub const MIN_MAC_NAME_LEN: usize = 1;
pub const MAX_MAC_NAME_LEN: usize = 64;
pub const MIN_PAIRING_FRAME_LEN: usize = 147;
pub const MAX_PAIRING_FRAME_LEN: usize = 210;

const MAGIC: [u8; 4] = *b"RPPK";
const VERSION: u8 = 1;
const FIXED_PAYLOAD_LEN: usize = 138;
const MIN_PAYLOAD_LEN: usize = FIXED_PAYLOAD_LEN + MIN_MAC_NAME_LEN;
const MAX_PAYLOAD_LEN: usize = FIXED_PAYLOAD_LEN + MAX_MAC_NAME_LEN;
const SESSION_ID_OFFSET: usize = 8;
const EXPIRY_OFFSET: usize = 24;
const MAC_ID_OFFSET: usize = 32;
const PUBLIC_KEY_OFFSET: usize = 48;
const SECRET_OFFSET: usize = 113;
const NAME_LENGTH_OFFSET: usize = 145;
const NAME_OFFSET: usize = 146;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingPayloadError {
    Truncated,
    TrailingBytes,
    BadMagic,
    UnknownVersion,
    NonZeroFlags,
    PayloadTooLarge,
    LengthMismatch,
    InvalidSessionId,
    InvalidExpiry,
    InvalidMacId,
    InvalidPublicKey,
    InvalidSecret,
    InvalidNameLength,
    InvalidUtf8,
    InvalidName,
}

impl Display for PairingPayloadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid v1 pairing payload: {self:?}")
    }
}

impl Error for PairingPayloadError {}

pub struct PairingPayload {
    session_id: [u8; PAIRING_SESSION_ID_LEN],
    expires_at_epoch_ms: u64,
    mac_id: [u8; PAIRING_MAC_ID_LEN],
    mac_identity_public_key: [u8; PAIRING_PUBLIC_KEY_LEN],
    pairing_secret: Zeroizing<[u8; PAIRING_SECRET_LEN]>,
    mac_name: String,
}

impl PairingPayload {
    pub fn try_new(
        session_id: [u8; PAIRING_SESSION_ID_LEN],
        expires_at_epoch_ms: u64,
        mac_id: [u8; PAIRING_MAC_ID_LEN],
        mac_identity_public_key: [u8; PAIRING_PUBLIC_KEY_LEN],
        pairing_secret: [u8; PAIRING_SECRET_LEN],
        mac_name: &str,
    ) -> Result<Self, PairingPayloadError> {
        validate_nonzero(&session_id, PairingPayloadError::InvalidSessionId)?;
        if expires_at_epoch_ms == 0 {
            return Err(PairingPayloadError::InvalidExpiry);
        }
        validate_nonzero(&mac_id, PairingPayloadError::InvalidMacId)?;
        validate_public_key(&mac_identity_public_key)?;
        validate_nonzero(&pairing_secret, PairingPayloadError::InvalidSecret)?;
        validate_name(mac_name)?;

        Ok(Self {
            session_id,
            expires_at_epoch_ms,
            mac_id,
            mac_identity_public_key,
            pairing_secret: Zeroizing::new(pairing_secret),
            mac_name: mac_name.to_owned(),
        })
    }

    #[must_use]
    pub const fn session_id(&self) -> &[u8; PAIRING_SESSION_ID_LEN] {
        &self.session_id
    }

    #[must_use]
    pub const fn expires_at_epoch_ms(&self) -> u64 {
        self.expires_at_epoch_ms
    }

    #[must_use]
    pub const fn mac_id(&self) -> &[u8; PAIRING_MAC_ID_LEN] {
        &self.mac_id
    }

    #[must_use]
    pub const fn mac_identity_public_key(&self) -> &[u8; PAIRING_PUBLIC_KEY_LEN] {
        &self.mac_identity_public_key
    }

    #[must_use]
    pub fn pairing_secret(&self) -> &[u8; PAIRING_SECRET_LEN] {
        &self.pairing_secret
    }

    #[must_use]
    pub fn mac_name(&self) -> &str {
        &self.mac_name
    }
}

#[must_use]
pub fn encode_pairing_payload(payload: &PairingPayload) -> Vec<u8> {
    let name = payload.mac_name.as_bytes();
    let payload_len = FIXED_PAYLOAD_LEN + name.len();
    let mut frame = vec![0_u8; PAIRING_HEADER_LEN + payload_len];
    frame[0..4].copy_from_slice(&MAGIC);
    frame[4] = VERSION;
    frame[5] = 0;
    frame[6..8].copy_from_slice(&(payload_len as u16).to_be_bytes());
    frame[SESSION_ID_OFFSET..EXPIRY_OFFSET].copy_from_slice(&payload.session_id);
    frame[EXPIRY_OFFSET..MAC_ID_OFFSET].copy_from_slice(&payload.expires_at_epoch_ms.to_be_bytes());
    frame[MAC_ID_OFFSET..PUBLIC_KEY_OFFSET].copy_from_slice(&payload.mac_id);
    frame[PUBLIC_KEY_OFFSET..SECRET_OFFSET].copy_from_slice(&payload.mac_identity_public_key);
    frame[SECRET_OFFSET..NAME_LENGTH_OFFSET].copy_from_slice(payload.pairing_secret());
    frame[NAME_LENGTH_OFFSET] = name.len() as u8;
    frame[NAME_OFFSET..].copy_from_slice(name);
    frame
}

pub fn decode_pairing_payload(frame: &[u8]) -> Result<PairingPayload, PairingPayloadError> {
    validate_header(frame)?;
    let declared_payload_len = usize::from(u16::from_be_bytes([frame[6], frame[7]]));
    let total_len = PAIRING_HEADER_LEN + declared_payload_len;
    match frame.len().cmp(&total_len) {
        std::cmp::Ordering::Less => return Err(PairingPayloadError::Truncated),
        std::cmp::Ordering::Greater => return Err(PairingPayloadError::TrailingBytes),
        std::cmp::Ordering::Equal => {}
    }

    let name_len = usize::from(frame[NAME_LENGTH_OFFSET]);
    if !(MIN_MAC_NAME_LEN..=MAX_MAC_NAME_LEN).contains(&name_len) {
        return Err(PairingPayloadError::InvalidNameLength);
    }
    if declared_payload_len != FIXED_PAYLOAD_LEN + name_len {
        return Err(PairingPayloadError::LengthMismatch);
    }
    let name_bytes = &frame[NAME_OFFSET..];
    let mac_name = std::str::from_utf8(name_bytes).map_err(|_| PairingPayloadError::InvalidUtf8)?;

    PairingPayload::try_new(
        read_array(frame, SESSION_ID_OFFSET),
        u64::from_be_bytes(read_array(frame, EXPIRY_OFFSET)),
        read_array(frame, MAC_ID_OFFSET),
        read_array(frame, PUBLIC_KEY_OFFSET),
        read_array(frame, SECRET_OFFSET),
        mac_name,
    )
}

fn validate_header(frame: &[u8]) -> Result<(), PairingPayloadError> {
    if frame.len() < PAIRING_HEADER_LEN {
        return Err(PairingPayloadError::Truncated);
    }
    if frame[0..4] != MAGIC {
        return Err(PairingPayloadError::BadMagic);
    }
    if frame[4] != VERSION {
        return Err(PairingPayloadError::UnknownVersion);
    }
    if frame[5] != 0 {
        return Err(PairingPayloadError::NonZeroFlags);
    }
    let declared_payload_len = usize::from(u16::from_be_bytes([frame[6], frame[7]]));
    if declared_payload_len > MAX_PAYLOAD_LEN {
        return Err(PairingPayloadError::PayloadTooLarge);
    }
    if declared_payload_len < MIN_PAYLOAD_LEN {
        return Err(PairingPayloadError::LengthMismatch);
    }
    Ok(())
}

fn validate_nonzero(bytes: &[u8], error: PairingPayloadError) -> Result<(), PairingPayloadError> {
    if bytes.iter().all(|byte| *byte == 0) {
        Err(error)
    } else {
        Ok(())
    }
}

fn validate_public_key(
    public_key: &[u8; PAIRING_PUBLIC_KEY_LEN],
) -> Result<(), PairingPayloadError> {
    if public_key[0] != 0x04 || PublicKey::from_sec1_bytes(public_key).is_err() {
        Err(PairingPayloadError::InvalidPublicKey)
    } else {
        Ok(())
    }
}

fn validate_name(mac_name: &str) -> Result<(), PairingPayloadError> {
    if !(MIN_MAC_NAME_LEN..=MAX_MAC_NAME_LEN).contains(&mac_name.len()) {
        return Err(PairingPayloadError::InvalidNameLength);
    }
    if mac_name.chars().any(char::is_control) {
        return Err(PairingPayloadError::InvalidName);
    }
    Ok(())
}

fn read_array<const N: usize>(frame: &[u8], offset: usize) -> [u8; N] {
    frame[offset..offset + N]
        .try_into()
        .expect("validated fixed pairing payload offset")
}

const _: () = {
    assert!(MIN_PAIRING_FRAME_LEN == PAIRING_HEADER_LEN + MIN_PAYLOAD_LEN);
    assert!(MAX_PAIRING_FRAME_LEN == PAIRING_HEADER_LEN + MAX_PAYLOAD_LEN);
    assert!(NAME_OFFSET == PAIRING_HEADER_LEN + FIXED_PAYLOAD_LEN);
};
