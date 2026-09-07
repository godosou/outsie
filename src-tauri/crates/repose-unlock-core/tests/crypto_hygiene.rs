use std::fs;

use aes::Aes256;
use zeroize::ZeroizeOnDrop;

#[test]
fn rustcrypto_aes_key_schedule_and_gcm_temporary_key_zeroization_are_enabled() {
    fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}
    assert_zeroize_on_drop::<Aes256>();

    let manifest = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read core Cargo.toml");
    assert!(
        manifest.lines().any(|line| {
            line.starts_with("aes-gcm =")
                && line.contains("\"aes\"")
                && line.contains("\"zeroize\"")
        }),
        "aes-gcm/zeroize must wipe its temporary GHASH key"
    );
    assert!(
        manifest
            .lines()
            .any(|line| { line.starts_with("aes =") && line.contains("features = [\"zeroize\"]") }),
        "aes/zeroize must wipe the AES-256 expanded key schedule"
    );
}
