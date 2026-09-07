use std::process::Command;

#[test]
fn compiled_service_reports_the_permanent_offline_gate_without_touching_launchd() {
    let output = Command::new(env!("CARGO_BIN_EXE_repose-unlock-service"))
        .arg("--health-check-deny-only")
        .output()
        .expect("service health process starts");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "repose-unlock-service: deny-only-v1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn unknown_arguments_fail_without_starting_a_listener() {
    let output = Command::new(env!("CARGO_BIN_EXE_repose-unlock-service"))
        .arg("--unknown")
        .output()
        .expect("service process starts");
    assert_eq!(output.status.code(), Some(64));
    assert!(output.stdout.is_empty());
}
