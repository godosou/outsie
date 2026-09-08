use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use repose_lib::debug_bluetooth_pairing::{
    BluetoothPairingPeer, BluetoothPairingPort, BluetoothPairingPortError,
    BluetoothPairingRadioState, DebugBluetoothPairingBackend, PAIRING_PENDING_CANDIDATE_NAME,
    PairingMaterial, PairingMaterialError, PairingMaterialSource,
};
use repose_lib::debug_pairing_material::OsPairingMaterialSource;
use repose_lib::unlock::{ComponentHealth, UnlockBackend, UnlockCapability, UnlockErrorCode};
use repose_unlock_core::pairing::decode_pairing_payload;
use std::sync::{Arc, Mutex};

const MACOS_BLE_SOURCE: &str = include_str!("../native/bluetooth_pairing.m");
const BUILD_SOURCE: &str = include_str!("../build.rs");
const MACOS_INFO: &str = include_str!("../Info.plist");

#[derive(Clone)]
struct FakePort {
    state: Arc<Mutex<FakePortState>>,
}

#[derive(Default)]
struct FakePortState {
    radio: BluetoothPairingRadioState,
    started_sessions: Vec<String>,
    stopped: usize,
    peer: Option<BluetoothPairingPeer>,
}

impl FakePort {
    fn powered_on() -> (Self, Arc<Mutex<FakePortState>>) {
        let state = Arc::new(Mutex::new(FakePortState {
            radio: BluetoothPairingRadioState::Ready,
            ..FakePortState::default()
        }));
        (
            Self {
                state: state.clone(),
            },
            state,
        )
    }
}

impl BluetoothPairingPort for FakePort {
    fn radio_state(&self) -> BluetoothPairingRadioState {
        self.state.lock().unwrap().radio
    }

    fn start_advertising(&mut self, session_id: &str) -> Result<(), BluetoothPairingPortError> {
        self.state
            .lock()
            .unwrap()
            .started_sessions
            .push(session_id.to_owned());
        Ok(())
    }

    fn stop_advertising(&mut self) {
        self.state.lock().unwrap().stopped += 1;
    }

    fn connected_peer(&self) -> Option<BluetoothPairingPeer> {
        self.state.lock().unwrap().peer.clone()
    }
}

struct FixedMaterialSource;

impl PairingMaterialSource for FixedMaterialSource {
    fn create(&mut self, now_epoch_ms: u64) -> Result<PairingMaterial, PairingMaterialError> {
        Ok(PairingMaterial {
            session_id: "00112233445566778899aabbccddeeff".into(),
            qr_payload: "repose://pair/v1/test-only-opaque-payload".into(),
            expires_at_epoch_ms: now_epoch_ms + 120_000,
        })
    }
}

fn backend(
    now_epoch_ms: u64,
) -> (
    DebugBluetoothPairingBackend<FakePort, FixedMaterialSource>,
    Arc<Mutex<FakePortState>>,
) {
    let (port, state) = FakePort::powered_on();
    (
        DebugBluetoothPairingBackend::new(port, FixedMaterialSource, move || now_epoch_ms),
        state,
    )
}

#[test]
fn debug_ble_can_be_ready_while_the_macos_authorization_gate_stays_closed() {
    let (mut backend, _) = backend(1_000);

    let snapshot = backend.unlock_status().unwrap();

    assert_eq!(snapshot.capability, UnlockCapability::Ready);
    assert!(!snapshot.authorization_gate.install_allowed);
    assert_eq!(snapshot.components.policy, ComponentHealth::NotChecked);
    assert_eq!(snapshot.components.plugin, ComponentHealth::NotChecked);
    assert_eq!(snapshot.components.service, ComponentHealth::Ready);
    assert_eq!(snapshot.components.transport, ComponentHealth::Ready);
}

#[test]
fn begin_pairing_starts_the_exact_ble_session_and_exposes_the_one_time_payload() {
    let (mut backend, state) = backend(1_000);

    let snapshot = backend.begin_pairing().unwrap();

    assert_eq!(
        state.lock().unwrap().started_sessions,
        ["00112233445566778899aabbccddeeff"]
    );
    let pending = snapshot.pending_pairing.expect("pending pairing");
    assert_eq!(pending.session_id, "00112233445566778899aabbccddeeff");
    assert_eq!(pending.candidate_name, PAIRING_PENDING_CANDIDATE_NAME);
    assert_eq!(
        pending.qr_payload,
        "repose://pair/v1/test-only-opaque-payload"
    );
    assert_eq!(pending.expires_at_epoch_ms, 121_000);
}

#[test]
fn confirmation_requires_a_peer_bound_to_the_current_session() {
    let (mut backend, state) = backend(1_000);
    backend.begin_pairing().unwrap();

    assert_eq!(
        backend
            .confirm_pairing("00112233445566778899aabbccddeeff")
            .unwrap_err()
            .code,
        UnlockErrorCode::BackendUnavailable
    );

    state.lock().unwrap().peer = Some(BluetoothPairingPeer {
        session_id: "stale-session".into(),
        device_id: "phone-stale".into(),
        display_name: "Wrong phone".into(),
    });
    assert_eq!(
        backend
            .confirm_pairing("00112233445566778899aabbccddeeff")
            .unwrap_err()
            .code,
        UnlockErrorCode::BackendUnavailable
    );
}

#[test]
fn a_matching_gatt_peer_can_be_confirmed_and_then_revoked() {
    let (mut backend, state) = backend(1_000);
    backend.begin_pairing().unwrap();
    state.lock().unwrap().peer = Some(BluetoothPairingPeer {
        session_id: "00112233445566778899aabbccddeeff".into(),
        device_id: "rmx3888-debug".into(),
        display_name: "realme GT5 Pro".into(),
    });

    let paired = backend
        .confirm_pairing("00112233445566778899aabbccddeeff")
        .unwrap();
    assert!(paired.pending_pairing.is_none());
    assert_eq!(paired.devices.len(), 1);
    assert_eq!(paired.devices[0].id, "rmx3888-debug");
    assert_eq!(paired.devices[0].display_name, "realme GT5 Pro");
    assert_eq!(state.lock().unwrap().stopped, 1);

    let revoked = backend.revoke_device("rmx3888-debug").unwrap();
    assert!(revoked.devices.is_empty());
    assert_eq!(
        backend.revoke_device("rmx3888-debug").unwrap_err().code,
        UnlockErrorCode::DeviceNotFound
    );
}

#[test]
fn expired_pairing_is_closed_before_confirmation() {
    let (mut backend, state) = backend(1_000);
    backend.begin_pairing().unwrap();
    backend.set_test_clock(|| 121_000);
    state.lock().unwrap().peer = Some(BluetoothPairingPeer {
        session_id: "00112233445566778899aabbccddeeff".into(),
        device_id: "rmx3888-debug".into(),
        display_name: "realme GT5 Pro".into(),
    });

    assert_eq!(
        backend
            .confirm_pairing("00112233445566778899aabbccddeeff")
            .unwrap_err()
            .code,
        UnlockErrorCode::PairingExpired
    );
    assert_eq!(state.lock().unwrap().stopped, 1);
    assert!(backend.unlock_status().unwrap().pending_pairing.is_none());
}

#[test]
fn powered_off_bluetooth_never_presents_a_ready_pairing_surface() {
    let (mut backend, state) = backend(1_000);
    state.lock().unwrap().radio = BluetoothPairingRadioState::PoweredOff;

    let snapshot = backend.unlock_status().unwrap();

    assert_eq!(snapshot.capability, UnlockCapability::ServiceUnavailable);
    assert_eq!(snapshot.components.transport, ComponentHealth::Unavailable);
    assert_eq!(
        backend.begin_pairing().unwrap_err().code,
        UnlockErrorCode::BackendUnavailable
    );
    assert!(state.lock().unwrap().started_sessions.is_empty());
}

#[test]
fn losing_bluetooth_while_pairing_stops_advertising_and_invalidates_the_qr() {
    let (mut backend, state) = backend(1_000);
    let started = backend.begin_pairing().unwrap();
    assert!(started.pending_pairing.is_some());

    state.lock().unwrap().radio = BluetoothPairingRadioState::PoweredOff;
    let unavailable = backend.unlock_status().unwrap();

    assert_eq!(unavailable.capability, UnlockCapability::ServiceUnavailable);
    assert!(unavailable.pending_pairing.is_none());
    assert_eq!(state.lock().unwrap().stopped, 1);
}

#[test]
fn macos_debug_build_contains_the_real_core_bluetooth_peripheral_boundary() {
    assert!(MACOS_BLE_SOURCE.contains("CBPeripheralManager"));
    assert!(MACOS_BLE_SOURCE.contains("A53E0001-7A6B-4D59-9F2E-5245504F5345"));
    assert!(MACOS_BLE_SOURCE.contains("A53E0002-7A6B-4D59-9F2E-5245504F5345"));
    assert!(MACOS_BLE_SOURCE.contains("A53E0003-7A6B-4D59-9F2E-5245504F5345"));
    assert!(BUILD_SOURCE.contains("native/bluetooth_pairing.m"));
    assert!(BUILD_SOURCE.contains("framework=CoreBluetooth"));
    assert!(MACOS_INFO.contains("NSBluetoothAlwaysUsageDescription"));
}

#[test]
fn macos_write_callback_rejects_noncanonical_batches_with_one_response() {
    let callback = source_between(
        MACOS_BLE_SOURCE,
        "didReceiveWriteRequests:(NSArray<CBATTRequest *> *)requests",
        "- (BOOL)startSession:",
    );

    assert!(callback.contains("requests.firstObject"));
    assert!(callback.contains("requests.count != 1"));
    assert!(callback.contains("request.offset != 0"));
    assert_eq!(
        callback.matches("respondToRequest:request").count(),
        1,
        "CoreBluetooth permits exactly one ATT response per write callback",
    );
}

#[test]
fn macos_read_callback_rejects_nonzero_offsets() {
    let callback = source_between(
        MACOS_BLE_SOURCE,
        "didReceiveReadRequest:(CBATTRequest *)request",
        "didReceiveWriteRequests:(NSArray<CBATTRequest *> *)requests",
    );

    assert!(callback.contains("request.offset != 0"));
    assert!(callback.contains("CBATTErrorInvalidOffset"));
}

#[test]
fn macos_ignores_stale_service_publication_callbacks() {
    let add_callback = source_between(
        MACOS_BLE_SOURCE,
        "didAddService:(CBService *)service",
        "didReceiveReadRequest:(CBATTRequest *)request",
    );
    let stop_session = source_between(MACOS_BLE_SOURCE, "- (void)stopSession", "@end");

    assert!(MACOS_BLE_SOURCE.contains("currentPublishedService"));
    assert!(add_callback.contains("service != self.currentPublishedService"));
    assert!(stop_session.contains("self.currentPublishedService = nil"));
}

#[test]
fn generated_debug_code_is_a_valid_bounded_rppk_payload() {
    let mut source = OsPairingMaterialSource::new("Repose MacBook Pro").unwrap();

    let material = source.create(1_800_000_000_000).unwrap();

    let encoded = material
        .qr_payload
        .strip_prefix("repose://pair/v1/")
        .expect("canonical URI prefix");
    let frame = URL_SAFE_NO_PAD.decode(encoded).unwrap();
    let decoded = decode_pairing_payload(&frame).unwrap();
    assert_eq!(decoded.expires_at_epoch_ms(), 1_800_000_120_000);
    assert_eq!(decoded.mac_name(), "Repose MacBook Pro");
    assert_eq!(material.session_id, hex(decoded.session_id()));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).expect("source start marker");
    let tail = &source[start..];
    let end = tail.find(end).expect("source end marker");
    &tail[..end]
}

#[test]
#[cfg(target_os = "macos")]
fn macos_console_native_fragment_and_lifecycle_simulation_without_radio() {
    use std::process::Command;
    let binary =
        std::env::temp_dir().join(format!("repose-console-native-test-{}", std::process::id()));
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_bluetooth_console.m");
    let build = Command::new("clang")
        .args(["-fobjc-arc", "-Wall", "-Wextra"])
        .arg(source)
        .args([
            "-framework",
            "Foundation",
            "-framework",
            "CoreBluetooth",
            "-o",
        ])
        .arg(&binary)
        .output()
        .expect("clang available for native macOS build");
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let result = Command::new(&binary)
        .output()
        .expect("run native simulation");
    let _ = std::fs::remove_file(binary);
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
