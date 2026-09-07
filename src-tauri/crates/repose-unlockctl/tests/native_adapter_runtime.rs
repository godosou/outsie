#![cfg(target_os = "macos")]

use std::fs;
use std::process::Command;

#[test]
fn objective_c_adapter_links_and_runs_against_fake_authorization_services() {
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let executable = std::env::temp_dir().join(format!(
        "repose-authdb-fake-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let compile = Command::new("/usr/bin/xcrun")
        .args([
            "--sdk",
            "macosx",
            "clang",
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-fno-objc-arc",
            "native/authorization_db_fake_test.m",
            "-framework",
            "CoreFoundation",
            "-o",
        ])
        .arg(&executable)
        .current_dir(crate_root)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "native fake-link compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&executable).env_clear().output().unwrap();
    let _ = fs::remove_file(&executable);
    assert!(
        run.status.success(),
        "native adapter fake test failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
