fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=native/macos.m");
        cc::Build::new()
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .compile("repose_macos");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=UserNotifications");
    }
    tauri_build::build()
}
