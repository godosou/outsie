fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=native/macos.m");
        println!("cargo:rerun-if-changed=native/work_console.m");
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
