use repose_lib::unlock::{
    CalibrationPhase, ComponentHealth, UnlockBackend, UnlockCapability, UnlockCommandError,
    UnlockCommandService, UnlockDiagnostics, UnlockErrorCode, UnlockSnapshot,
    closed_unlock_diagnostics, closed_unlock_snapshot,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const UNLOCK_SOURCE: &str = include_str!("../src/unlock.rs");
const LIB_SOURCE: &str = include_str!("../src/lib.rs");
const BUILD_SOURCE: &str = include_str!("../build.rs");
const DEFAULT_CAPABILITY: &str = include_str!("../capabilities/default.json");
const UNLOCK_CAPABILITY: &str = include_str!("../capabilities/unlock-main.json");

struct FakeSafeBackend {
    calls: Arc<AtomicUsize>,
}

impl FakeSafeBackend {
    fn ready() -> (Self, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                calls: calls.clone(),
            },
            calls,
        )
    }

    fn snapshot(&self) -> UnlockSnapshot {
        let mut snapshot = closed_unlock_snapshot();
        snapshot.capability = UnlockCapability::Ready;
        snapshot.components.service = ComponentHealth::Ready;
        snapshot.components.transport = ComponentHealth::Ready;
        snapshot
    }

    fn called(&self) {
        self.calls.fetch_add(1, Ordering::Relaxed);
    }
}

impl UnlockBackend for FakeSafeBackend {
    fn unlock_status(&mut self) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.called();
        Ok(self.snapshot())
    }

    fn begin_pairing(&mut self) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.called();
        Ok(self.snapshot())
    }

    fn confirm_pairing(&mut self, _session_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.called();
        Ok(self.snapshot())
    }

    fn begin_calibration(
        &mut self,
        _device_id: &str,
    ) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.called();
        Ok(self.snapshot())
    }

    fn revoke_device(&mut self, _device_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.called();
        Ok(self.snapshot())
    }

    fn open_unlock_diagnostics(&mut self) -> Result<UnlockDiagnostics, UnlockCommandError> {
        self.called();
        Ok(closed_unlock_diagnostics())
    }
}

#[test]
fn closed_backend_reports_an_honest_non_installable_snapshot() {
    let service = UnlockCommandService::closed();
    let snapshot = service
        .unlock_status("main")
        .expect("closed status is readable");

    assert_eq!(snapshot.capability, UnlockCapability::GateClosed);
    assert!(!snapshot.authorization_gate.install_allowed);
    assert_eq!(snapshot.components.policy, ComponentHealth::NotChecked);
    assert_eq!(snapshot.components.plugin, ComponentHealth::NotChecked);
    assert_eq!(snapshot.components.service, ComponentHealth::NotChecked);
    assert_eq!(snapshot.components.transport, ComponentHealth::NotChecked);
    assert!(snapshot.devices.is_empty());
    assert!(snapshot.pending_pairing.is_none());
    assert_eq!(snapshot.calibration.phase, CalibrationPhase::Unavailable);
}

#[test]
fn non_main_callers_are_rejected_before_the_backend_is_touched() {
    let (backend, calls) = FakeSafeBackend::ready();
    let service = UnlockCommandService::new(backend);

    let error = service
        .begin_pairing("break-0")
        .expect_err("break window must be denied");
    assert_eq!(error.code, UnlockErrorCode::CallerNotAllowed);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn caller_authorization_precedes_identifier_validation() {
    let (backend, calls) = FakeSafeBackend::ready();
    let service = UnlockCommandService::new(backend);

    let error = service
        .revoke_device("break-0", "../invalid")
        .expect_err("an unauthorized caller gets no request-validation oracle");
    assert_eq!(error.code, UnlockErrorCode::CallerNotAllowed);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn opaque_identifiers_are_bounded_before_dispatch() {
    let (backend, calls) = FakeSafeBackend::ready();
    let service = UnlockCommandService::new(backend);

    let too_long = "a".repeat(65);
    for invalid in ["", "../phone", "phone id", "é", too_long.as_str()] {
        let error = service
            .revoke_device("main", invalid)
            .expect_err("invalid identifier must be denied");
        assert_eq!(error.code, UnlockErrorCode::InvalidRequest);
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn a_valid_main_window_request_dispatches_once() {
    let (backend, calls) = FakeSafeBackend::ready();
    let service = UnlockCommandService::new(backend);

    service
        .revoke_device("main", "phone_1")
        .expect("valid request reaches the bounded backend");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn every_unconnected_mutation_fails_with_a_fixed_code() {
    let service = UnlockCommandService::closed();

    assert_eq!(
        service.begin_pairing("main").unwrap_err().code,
        UnlockErrorCode::BackendUnavailable
    );
    assert_eq!(
        service.confirm_pairing("main", "pair_1").unwrap_err().code,
        UnlockErrorCode::BackendUnavailable
    );
    assert_eq!(
        service
            .begin_calibration("main", "phone_1")
            .unwrap_err()
            .code,
        UnlockErrorCode::BackendUnavailable
    );
    assert_eq!(
        service.revoke_device("main", "phone_1").unwrap_err().code,
        UnlockErrorCode::BackendUnavailable
    );
}

#[test]
fn command_error_codes_serialize_to_the_renderer_contract() {
    let cases = [
        (UnlockErrorCode::CallerNotAllowed, "callerNotAllowed"),
        (UnlockErrorCode::InvalidRequest, "invalidRequest"),
        (UnlockErrorCode::BackendUnavailable, "backendUnavailable"),
        (UnlockErrorCode::Busy, "busy"),
        (UnlockErrorCode::PairingExpired, "pairingExpired"),
        (UnlockErrorCode::DeviceNotFound, "deviceNotFound"),
    ];

    for (code, expected) in cases {
        assert_eq!(
            serde_json::to_value(UnlockCommandError { code })
                .expect("command errors must serialize")["code"],
            expected,
        );
    }
}

#[test]
fn diagnostics_are_structured_and_redacted() {
    let service = UnlockCommandService::closed();
    let diagnostics = service
        .open_unlock_diagnostics("main")
        .expect("closed diagnostics remain available");
    let debug = format!("{diagnostics:?}").to_ascii_lowercase();

    for forbidden in [
        "pairingsecret",
        "private key",
        "qrpayload",
        "/users/",
        "/private/",
    ] {
        assert!(!debug.contains(forbidden), "diagnostics leaked {forbidden}");
    }
}

#[test]
fn tauri_surface_and_acl_are_exactly_scoped() {
    let commands = [
        "unlock_status",
        "begin_pairing",
        "confirm_pairing",
        "begin_calibration",
        "revoke_device",
        "open_unlock_diagnostics",
    ];
    for command in commands {
        assert!(LIB_SOURCE.contains(command), "missing handler {command}");
        assert!(
            BUILD_SOURCE.contains(command),
            "missing app ACL manifest {command}"
        );
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(UNLOCK_CAPABILITY.contains(&permission));
        assert!(!DEFAULT_CAPABILITY.contains(&permission));
    }
    assert!(UNLOCK_CAPABILITY.contains(r#""windows": ["main"]"#));
    assert!(DEFAULT_CAPABILITY.contains(r#""windows": ["main", "break-*"]"#));
}

#[test]
fn lifecycle_commands_remain_available_to_the_timer_renderer() {
    let capability: serde_json::Value =
        serde_json::from_str(DEFAULT_CAPABILITY).expect("valid default capability");
    let windows = capability["windows"]
        .as_array()
        .expect("capability windows");
    assert!(windows.iter().any(|window| window == "main"));
    let permissions = capability["permissions"]
        .as_array()
        .expect("capability permissions");

    // Registering the phone-key app manifest enables command ACL enforcement for
    // the timer too. Both replay and acknowledgement must survive that change.
    for command in ["get_lifecycle_snapshot", "acknowledge_lifecycle_interval"] {
        assert!(LIB_SOURCE.contains(command), "missing handler {command}");
        assert!(
            BUILD_SOURCE.contains(&format!("\"{command}\"")),
            "lifecycle command missing from app ACL manifest: {command}"
        );
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(
            permissions.iter().any(|entry| entry == &permission),
            "timer renderer lacks lifecycle permission: {permission}"
        );
    }
}

#[test]
fn unlock_module_has_no_generic_system_or_secret_bearing_surface() {
    for forbidden in [
        "std::process",
        "Command::new",
        "PathBuf",
        "authorizationdb",
        "system.login",
        "raw_ble",
        "sign_bytes",
    ] {
        assert!(
            !UNLOCK_SOURCE.contains(forbidden),
            "forbidden surface: {forbidden}"
        );
    }
}
