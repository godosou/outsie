use std::fs;
use std::path::PathBuf;

use repose_unlock_core::pairing::{
    BLE_CONTROL_CHARACTERISTIC_UUID, BLE_SERVICE_UUID, BLE_STATUS_CHARACTERISTIC_UUID,
    MAX_PAIRING_FRAME_LEN, PairingPayload, PairingPayloadError, decode_pairing_payload,
    encode_pairing_payload,
};

const SESSION_ID: [u8; 16] = [
    0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
];
const MAC_ID: [u8; 16] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
];
const PAIRING_SECRET: [u8; 32] = [
    0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf,
    0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf,
];
const MAC_PUBLIC_KEY: [u8; 65] = [
    0x04, 0xe2, 0x53, 0x4a, 0x35, 0x32, 0xd0, 0x8f, 0xbb, 0xa0, 0x2d, 0xde, 0x65, 0x9e, 0xe6, 0x2b,
    0xd0, 0x03, 0x1f, 0xe2, 0xdb, 0x78, 0x55, 0x96, 0xef, 0x50, 0x93, 0x02, 0x44, 0x6b, 0x03, 0x08,
    0x52, 0xe0, 0xf1, 0x57, 0x5a, 0x4c, 0x63, 0x3c, 0xc7, 0x19, 0xdf, 0xee, 0x5f, 0xda, 0x86, 0x2d,
    0x76, 0x4e, 0xfc, 0x96, 0xc3, 0xf3, 0x0e, 0xe0, 0x05, 0x5c, 0x42, 0xc2, 0x3f, 0x18, 0x4e, 0xd8,
    0xc6,
];

#[test]
fn rust_encodes_and_decodes_the_shared_pairing_fixture_byte_for_byte() {
    let fixture = fixture();
    let payload = PairingPayload::try_new(
        SESSION_ID,
        1_800_000_000_000,
        MAC_ID,
        MAC_PUBLIC_KEY,
        PAIRING_SECRET,
        "Repose MacBook Pro",
    )
    .unwrap();

    assert_eq!(fixture.len(), 164);
    assert_eq!(encode_pairing_payload(&payload), fixture);

    let decoded = decode_pairing_payload(&fixture).unwrap();
    assert_eq!(decoded.session_id(), &SESSION_ID);
    assert_eq!(decoded.expires_at_epoch_ms(), 1_800_000_000_000);
    assert_eq!(decoded.mac_id(), &MAC_ID);
    assert_eq!(decoded.mac_identity_public_key(), &MAC_PUBLIC_KEY);
    assert_eq!(decoded.pairing_secret(), &PAIRING_SECRET);
    assert_eq!(decoded.mac_name(), "Repose MacBook Pro");
    assert_eq!(encode_pairing_payload(&decoded), fixture);
}

#[test]
fn pairing_parser_rejects_noncanonical_lengths_headers_and_trailing_bytes() {
    let fixture = fixture();
    let mutations = [
        mutate(&fixture, 0, fixture[0] ^ 1),
        mutate(&fixture, 4, 2),
        mutate(&fixture, 5, 1),
        mutate(&fixture, 7, fixture[7] - 1),
        mutate(&fixture, 7, fixture[7] + 1),
        mutate(&fixture, 145, 17),
        fixture[..fixture.len() - 1].to_vec(),
        [fixture.as_slice(), &[0]].concat(),
    ];

    for mutation in mutations {
        assert!(decode_pairing_payload(&mutation).is_err());
    }
    assert!(matches!(
        decode_pairing_payload(&fixture[..fixture.len() - 1]),
        Err(PairingPayloadError::Truncated)
    ));
    assert!(matches!(
        decode_pairing_payload(&[fixture.as_slice(), &[0]].concat()),
        Err(PairingPayloadError::TrailingBytes)
    ));
}

#[test]
fn pairing_codec_rejects_invalid_identifiers_key_secret_and_utf8_name() {
    let fixture = fixture();
    let mut cases = Vec::new();
    cases.push(fixture.clone().tap_mut(|bytes| bytes[8..24].fill(0)));
    cases.push(fixture.clone().tap_mut(|bytes| bytes[24..32].fill(0)));
    cases.push(fixture.clone().tap_mut(|bytes| bytes[32..48].fill(0)));
    cases.push(fixture.clone().tap_mut(|bytes| bytes[48] = 3));
    cases.push(fixture.clone().tap_mut(|bytes| {
        bytes[48..113].fill(0);
        bytes[48] = 4;
    }));
    cases.push(fixture.clone().tap_mut(|bytes| bytes[113..145].fill(0)));
    cases.push(fixture.clone().tap_mut(|bytes| bytes[146] = 0xff));
    cases.push(fixture.clone().tap_mut(|bytes| bytes[146] = b'\n'));

    for mutation in cases {
        assert!(decode_pairing_payload(&mutation).is_err());
    }
}

#[test]
fn pairing_encoder_enforces_utf8_byte_length_instead_of_character_count() {
    let name = "钥".repeat(22);
    let result = PairingPayload::try_new(
        SESSION_ID,
        1_800_000_000_000,
        MAC_ID,
        MAC_PUBLIC_KEY,
        PAIRING_SECRET,
        &name,
    );

    assert!(matches!(
        result,
        Err(PairingPayloadError::InvalidNameLength)
    ));
}

#[test]
fn gatt_identifiers_are_fixed_for_both_platforms() {
    assert_eq!(BLE_SERVICE_UUID, "A53E0001-7A6B-4D59-9F2E-5245504F5345");
    assert_eq!(
        BLE_CONTROL_CHARACTERISTIC_UUID,
        "A53E0002-7A6B-4D59-9F2E-5245504F5345"
    );
    assert_eq!(
        BLE_STATUS_CHARACTERISTIC_UUID,
        "A53E0003-7A6B-4D59-9F2E-5245504F5345"
    );
    assert_eq!(MAX_PAIRING_FRAME_LEN, 210);
}

fn fixture() -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../protocol/fixtures/v1/pairing-payload.bin"),
    )
    .expect("read pairing payload fixture")
}

fn mutate(input: &[u8], offset: usize, value: u8) -> Vec<u8> {
    let mut output = input.to_vec();
    output[offset] = value;
    output
}

trait TapMut: Sized {
    fn tap_mut(mut self, action: impl FnOnce(&mut Self)) -> Self {
        action(&mut self);
        self
    }
}

impl<T> TapMut for T {}
