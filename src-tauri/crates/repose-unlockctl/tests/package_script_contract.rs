const PACKAGE: &str = include_str!("../../../../scripts/package-macos-auth-components.sh");
const VERIFY: &str = include_str!("../../../../scripts/verify-macos-auth-artifacts.sh");

#[test]
fn generic_package_verification_never_executes_caller_supplied_service() {
    assert!(!VERIFY.contains("--health-check-deny-only"));
    assert!(!VERIFY.contains("\"$service\" --"));
    assert!(
        !VERIFY.contains("bundle_smoke"),
        "generic verification must not dlopen a caller-supplied plugin"
    );
}

#[test]
fn only_the_nonroot_build_pipeline_runs_health_on_its_own_cargo_output() {
    assert!(PACKAGE.contains("refusing to build a plan-only package as root"));
    assert!(
        PACKAGE.contains("\"$repository_root/src-tauri/target/release/repose-unlock-service\"")
    );
    assert!(PACKAGE.contains("--health-check-deny-only"));
    assert!(!PACKAGE.contains("\"$staging/bin/ai.repose.unlockd\" --health"));
}
