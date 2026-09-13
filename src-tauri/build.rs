fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=native/macos.m");
        println!("cargo:rerun-if-changed=native/work_console.m");
        // The helper scripts and binaries are bundled as resources, and
        // tauri-build stages them when THIS script runs. Without these lines a
        // Rust-only change rebuilt the app around a stale copy of the bridge:
        // on 2026-09-12 an installed bundle carried a permit-bridge.sh an hour
        // older than the source.
        for f in [
            "../tools/ble-spike/mac/permit-bridge.sh",
            "../tools/ble-spike/mac/presence-pipeline.sh",
            "../tools/ble-spike/mac/rssi-scan",
            "../tools/ble-spike/mac/presence-verify",
            "../tools/ble-spike/mac/pair-with-phone",
            "../tools/ble-spike/mac/state-advertise",
            "../tools/ble-spike/mac/send-catalogue",
            "../tools/lib/run-root.sh",
        ] {
            println!("cargo:rerun-if-changed={f}");
        }
        cc::Build::new()
            .file("native/macos.m")
            // Pressing a key in another application is the one thing here that
            // cannot be done from Rust: it needs AXIsProcessTrusted, the
            // running-application list, and CGEvent posting with a Carbon
            // keycode. Ported from the prototype branch unchanged except for
            // the session guard it already had.
            .file("native/work_console.m")
            .flag("-fobjc-arc")
            .compile("repose_macos");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=UserNotifications");
        println!("cargo:rustc-link-lib=framework=Carbon");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
        println!("cargo:rustc-link-lib=framework=UniformTypeIdentifiers");
    }
    tauri_build::build()
}
