fn main() {
    println!("cargo:rerun-if-changed=native/authorization_db.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/authorization_db.m")
            .flag("-fno-objc-arc")
            .warnings(true)
            .extra_warnings(true)
            .compile("repose_unlockctl_authorization_db");
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
}
