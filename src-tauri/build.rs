fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=native/macos.m");
        println!("cargo:rerun-if-changed=native/work_console.m");
        println!("cargo:rerun-if-changed=native/bluetooth_pairing.m");
        cc::Build::new()
            .file("native/macos.m")
            .file("native/work_console.m")
            .file("native/bluetooth_pairing.m")
            .flag("-fobjc-arc")
            .compile("repose_macos");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Carbon");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=UserNotifications");
        println!("cargo:rustc-link-lib=framework=CoreBluetooth");
    }
    const COMMANDS: &[&str] = &[
        "console_status",
        "console_save",
        "console_reset",
        "console_start",
        "console_stop",
        "console_run",
        "console_cancel",
        "console_accessibility",
        "set_status",
        "set_preferences",
        "get_lifecycle_snapshot",
        "acknowledge_lifecycle_interval",
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
