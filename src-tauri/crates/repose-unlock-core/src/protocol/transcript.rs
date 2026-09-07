use sha2::{Digest, Sha256};

use crate::protocol::messages::Response;
use crate::protocol::wire::{
    CHALLENGE_FRAME_LEN, RESPONSE_PREFIX_LEN, RESPONSE_SIGNED_PREFIX_LEN, encode_response,
};

pub const KDF_CONTEXT_LABEL: &[u8] = b"repose-unlock-v1 kdf-context phone-response";
pub const HKDF_SALT_LABEL: &[u8] = b"repose-unlock-v1 hkdf-salt";
pub const MAC_TO_PHONE_KEY_LABEL: &[u8] = b"repose-unlock-v1 key mac-to-phone";
pub const MAC_TO_PHONE_NONCE_LABEL: &[u8] = b"repose-unlock-v1 nonce mac-to-phone";
pub const PHONE_TO_MAC_KEY_LABEL: &[u8] = b"repose-unlock-v1 key phone-to-mac";
pub const PHONE_TO_MAC_NONCE_LABEL: &[u8] = b"repose-unlock-v1 nonce phone-to-mac";
pub const PHONE_TO_MAC_AAD_LABEL: &[u8] = b"repose-unlock-v1 aad phone-to-mac";
pub const PHONE_TO_MAC_PROOF_LABEL: &[u8] = b"repose-unlock-v1 proof phone-to-mac";
pub const PHONE_TO_MAC_SIGNATURE_LABEL: &[u8] = b"repose-unlock-v1 signature phone-to-mac";

pub(crate) fn context_hash(
    challenge_frame: &[u8; CHALLENGE_FRAME_LEN],
    response: &Response,
) -> [u8; 32] {
    let encoded = encode_response(response);
    hash_parts(&[
        KDF_CONTEXT_LABEL,
        challenge_frame,
        &encoded[..RESPONSE_PREFIX_LEN],
    ])
}

pub(crate) fn salt(context_hash: &[u8; 32]) -> [u8; 32] {
    hash_parts(&[HKDF_SALT_LABEL, context_hash])
}

pub(crate) fn aad(challenge_frame: &[u8; CHALLENGE_FRAME_LEN], response: &Response) -> Vec<u8> {
    let encoded = encode_response(response);
    let mut output = Vec::with_capacity(
        PHONE_TO_MAC_AAD_LABEL.len() + CHALLENGE_FRAME_LEN + RESPONSE_PREFIX_LEN,
    );
    output.extend_from_slice(PHONE_TO_MAC_AAD_LABEL);
    output.extend_from_slice(challenge_frame);
    output.extend_from_slice(&encoded[..RESPONSE_PREFIX_LEN]);
    output
}

pub(crate) fn proof(context_hash: &[u8; 32]) -> [u8; 32] {
    hash_parts(&[PHONE_TO_MAC_PROOF_LABEL, context_hash])
}

pub(crate) fn signature_hash(
    challenge_frame: &[u8; CHALLENGE_FRAME_LEN],
    response: &Response,
) -> [u8; 32] {
    let encoded = encode_response(response);
    hash_parts(&[
        PHONE_TO_MAC_SIGNATURE_LABEL,
        challenge_frame,
        &encoded[..RESPONSE_SIGNED_PREFIX_LEN],
    ])
}

fn hash_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}
