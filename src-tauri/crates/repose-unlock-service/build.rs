fn main() {
    println!("cargo:rerun-if-changed=native/peer_identity.c");
    println!("cargo:rerun-if-changed=native/listener_identity.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/peer_identity.c")
            .file("native/listener_identity.c")
            .warnings(true)
            .warnings_into_errors(true)
            .compile("repose_peer_identity");
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=bsm");
        println!("cargo:rustc-link-lib=proc");
    }
}
