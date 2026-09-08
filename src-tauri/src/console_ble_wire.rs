//! Authenticated BLE application packets; no platform I/O or key persistence.
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

pub const HEADER_LEN: usize = 42;
pub const MAX_PACKET: usize = 256 * 1024;
pub const INFO: &[u8] = b"repose-console-ble-v1";

pub struct BleCipher {
    key: Zeroizing<[u8; 32]>,
    session: [u8; 16],
    challenge: [u8; 16],
    send_direction: u8,
    sent: u64,
    received: u64,
}
impl BleCipher {
    pub fn new(secret: &[u8; 32], session: [u8; 16], challenge: [u8; 16], server: bool) -> Self {
        let mut key = Zeroizing::new([0u8; 32]);
        Hkdf::<Sha256>::new(Some(&challenge), secret)
            .expand(INFO, key.as_mut())
            .expect("fixed HKDF output length");
        Self {
            key,
            session,
            challenge,
            send_direction: u8::from(server),
            sent: 0,
            received: 0,
        }
    }
    pub fn session(packet: &[u8]) -> Result<[u8; 16], String> {
        if packet.len() < HEADER_LEN + 16 || packet.len() > MAX_PACKET || packet[0] != 1 {
            return Err("Invalid BLE packet".into());
        }
        Ok(packet[2..18].try_into().unwrap())
    }
    pub fn encrypt(&mut self, plain: &[u8]) -> Result<Vec<u8>, String> {
        if plain.len() > MAX_PACKET - HEADER_LEN - 16 {
            return Err("BLE response too large".into());
        }
        let counter = self.sent.checked_add(1).ok_or("BLE session exhausted")?;
        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend([1, self.send_direction]);
        header.extend(self.session);
        header.extend(self.challenge);
        header.extend(counter.to_be_bytes());
        let mut nonce = [0; 12];
        nonce[0] = self.send_direction;
        nonce[4..].copy_from_slice(&counter.to_be_bytes());
        let cipher =
            Aes256Gcm::new_from_slice(self.key.as_ref()).map_err(|_| "BLE key unavailable")?;
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plain,
                    aad: &header,
                },
            )
            .map_err(|_| "BLE encryption failed")?;
        self.sent = counter;
        header.extend(encrypted);
        Ok(header)
    }
    pub fn decrypt(&mut self, packet: &[u8]) -> Result<Vec<u8>, String> {
        let session = Self::session(packet)?;
        if session != self.session
            || packet[1] != (1 - self.send_direction)
            || packet[18..34] != self.challenge
        {
            return Err("BLE session mismatch".into());
        }
        let counter = u64::from_be_bytes(packet[34..42].try_into().unwrap());
        if self.received.checked_add(1) != Some(counter) {
            return Err("BLE replay or out-of-order packet".into());
        }
        let mut nonce = [0; 12];
        nonce[0] = packet[1];
        nonce[4..].copy_from_slice(&counter.to_be_bytes());
        let cipher =
            Aes256Gcm::new_from_slice(self.key.as_ref()).map_err(|_| "BLE key unavailable")?;
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &packet[HEADER_LEN..],
                    aad: &packet[..HEADER_LEN],
                },
            )
            .map_err(|_| "BLE authentication failed")?;
        self.received = counter;
        Ok(plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_direction_challenge_and_authentication_are_bound() {
        let mut phone = BleCipher::new(&[2; 32], [1; 16], [3; 16], false);
        let mut mac = BleCipher::new(&[2; 32], [1; 16], [3; 16], true);
        let packet = phone.encrypt(br#"{"type":"status"}"#).unwrap();
        let mut corrupt = packet.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(mac.decrypt(&corrupt).is_err());
        assert_eq!(mac.decrypt(&packet).unwrap(), br#"{"type":"status"}"#);
        assert!(mac.decrypt(&packet).is_err());
        assert!(phone.decrypt(&packet).is_err());
        assert!(
            BleCipher::new(&[2; 32], [1; 16], [4; 16], true)
                .decrypt(&packet)
                .is_err()
        );
        assert!(
            BleCipher::new(&[9; 32], [1; 16], [3; 16], true)
                .decrypt(&packet)
                .is_err()
        );
        let reply = mac.encrypt(b"response").unwrap();
        assert_eq!(phone.decrypt(&reply).unwrap(), b"response");
    }
    #[test]
    fn shared_vector_and_bounds() {
        let mut phone = BleCipher::new(&[2; 32], [1; 16], [3; 16], false);
        let packet = phone.encrypt(br#"{"type":"status"}"#).unwrap();
        let vector: serde_json::Value =
            serde_json::from_str(include_str!("../../protocol/fixtures/console-ble-v1.json"))
                .unwrap();
        let hex: String = packet.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, vector["requestHex"]);
        assert!(phone.encrypt(&vec![0; MAX_PACKET]).is_err());
        assert!(phone.decrypt(&[0; 10]).is_err());
    }
}
