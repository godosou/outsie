#![forbid(unsafe_code)]

use crate::debug_bluetooth_pairing::{
    PairingMaterial, PairingMaterialError, PairingMaterialSource,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::SigningKey;
use rand_core::{OsRng, RngCore};
use repose_unlock_core::pairing::{PairingPayload, encode_pairing_payload};

const PAIRING_LIFETIME_MS: u64 = 120_000;

pub struct OsPairingMaterialSource {
    mac_id: [u8; 16],
    identity_key: SigningKey,
    mac_name: String,
}

impl OsPairingMaterialSource {
    pub fn new(mac_name: &str) -> Result<Self, PairingMaterialError> {
        if mac_name.is_empty() || mac_name.len() > 64 || mac_name.chars().any(char::is_control) {
            return Err(PairingMaterialError::InvalidMacName);
        }
        Ok(Self {
            mac_id: random_nonzero(),
            identity_key: SigningKey::random(&mut OsRng),
            mac_name: mac_name.to_owned(),
        })
    }
}

impl PairingMaterialSource for OsPairingMaterialSource {
    fn create(&mut self, now_epoch_ms: u64) -> Result<PairingMaterial, PairingMaterialError> {
        let session_id = random_nonzero();
        let pairing_secret = random_nonzero();
        let expires_at_epoch_ms = now_epoch_ms
            .checked_add(PAIRING_LIFETIME_MS)
            .ok_or(PairingMaterialError::ExpiryOverflow)?;
        let public_key: [u8; 65] = self
            .identity_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .try_into()
            .map_err(|_| PairingMaterialError::InvalidPayload)?;
        let payload = PairingPayload::try_new(
            session_id,
            expires_at_epoch_ms,
            self.mac_id,
            public_key,
            pairing_secret,
            &self.mac_name,
        )
        .map_err(|_| PairingMaterialError::InvalidPayload)?;
        let encoded = encode_pairing_payload(&payload);
        Ok(PairingMaterial {
            session_id: encode_hex(&session_id),
            qr_payload: format!("repose://pair/v1/{}", URL_SAFE_NO_PAD.encode(encoded)),
            expires_at_epoch_ms,
        })
    }
}

fn random_nonzero<const N: usize>() -> [u8; N] {
    loop {
        let mut bytes = [0_u8; N];
        OsRng.fill_bytes(&mut bytes);
        if bytes.iter().any(|byte| *byte != 0) {
            return bytes;
        }
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
