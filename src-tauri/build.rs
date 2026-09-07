fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .compile("repose_macos");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=UserNotifications");
    }
    const COMMANDS: &[&str] = &[
        "set_status",
        "set_preferences",
        "postpone_break",
        "notify_user",
        "open_security_settings",
        "unlock_status",
        "begin_pairing",
        "confirm_pairing",
        "begin_calibration",
        "revoke_device",
        "open_unlock_diagnostics",
    ];
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("failed to build Tauri command permissions")
}
