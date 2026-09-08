#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{State, WebviewWindow};

const VALIDATION_RECORD: &str = "docs/validation/macos-authorization-results.md";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthorizationGateState {
    NotRun,
    Failed,
    Passed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ComponentHealth {
    NotChecked,
    Missing,
    Ready,
    Degraded,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnlockCapability {
    Ready,
    GateClosed,
    ServiceUnavailable,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompanionPlatform {
    Android,
    Ios,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CalibrationPhase {
    Idle,
    CollectingNear,
    CollectingFar,
    Complete,
    OverlapRejected,
    Unavailable,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnlockLimitation {
    Task8GateClosed,
    RelayRisk,
    StolenUnlockedPhone,
    BluetoothOff,
    AndroidForceStop,
    PasswordFallback,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationGate {
    pub state: AuthorizationGateState,
    pub evidence_record: String,
    pub install_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentHealthSnapshot {
    pub policy: ComponentHealth,
    pub plugin: ComponentHealth,
    pub service: ComponentHealth,
    pub transport: ComponentHealth,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub id: String,
    pub display_name: String,
    pub platform: CompanionPlatform,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSession {
    pub session_id: String,
    pub candidate_name: String,
    pub qr_payload: String,
    pub expires_at_epoch_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSnapshot {
    pub device_id: Option<String>,
    pub phase: CalibrationPhase,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSnapshot {
    pub schema_version: u8,
    pub capability: UnlockCapability,
    pub authorization_gate: AuthorizationGate,
    pub components: ComponentHealthSnapshot,
    pub devices: Vec<PairedDevice>,
    pub pending_pairing: Option<PairingSession>,
    pub calibration: CalibrationSnapshot,
    pub limitations: Vec<UnlockLimitation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticsSummary {
    GateClosed,
    BackendUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockDiagnostics {
    pub schema_version: u8,
    pub summary: DiagnosticsSummary,
    pub authorization_gate: AuthorizationGateState,
    pub components: ComponentHealthSnapshot,
    pub validation_record: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnlockErrorCode {
    CallerNotAllowed,
    InvalidRequest,
    BackendUnavailable,
    Busy,
    PairingExpired,
    DeviceNotFound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UnlockCommandError {
    pub code: UnlockErrorCode,
}

impl UnlockCommandError {
    const fn new(code: UnlockErrorCode) -> Self {
        Self { code }
    }
}

pub fn closed_unlock_snapshot() -> UnlockSnapshot {
    UnlockSnapshot {
        schema_version: 1,
        capability: UnlockCapability::GateClosed,
        authorization_gate: AuthorizationGate {
            state: AuthorizationGateState::NotRun,
            evidence_record: VALIDATION_RECORD.to_owned(),
            install_allowed: false,
        },
        components: ComponentHealthSnapshot {
            policy: ComponentHealth::NotChecked,
            plugin: ComponentHealth::NotChecked,
            service: ComponentHealth::NotChecked,
            transport: ComponentHealth::NotChecked,
        },
        devices: Vec::new(),
        pending_pairing: None,
        calibration: CalibrationSnapshot {
            device_id: None,
            phase: CalibrationPhase::Unavailable,
        },
        limitations: vec![
            UnlockLimitation::Task8GateClosed,
            UnlockLimitation::RelayRisk,
            UnlockLimitation::StolenUnlockedPhone,
            UnlockLimitation::BluetoothOff,
            UnlockLimitation::AndroidForceStop,
            UnlockLimitation::PasswordFallback,
        ],
    }
}

pub fn closed_unlock_diagnostics() -> UnlockDiagnostics {
    let snapshot = closed_unlock_snapshot();
    UnlockDiagnostics {
        schema_version: 1,
        summary: DiagnosticsSummary::GateClosed,
        authorization_gate: snapshot.authorization_gate.state,
        components: snapshot.components,
        validation_record: VALIDATION_RECORD.to_owned(),
    }
}

pub trait UnlockBackend: Send {
    fn unlock_status(&mut self) -> Result<UnlockSnapshot, UnlockCommandError>;
    fn begin_pairing(&mut self) -> Result<UnlockSnapshot, UnlockCommandError>;
    fn confirm_pairing(&mut self, session_id: &str) -> Result<UnlockSnapshot, UnlockCommandError>;
    fn begin_calibration(&mut self, device_id: &str) -> Result<UnlockSnapshot, UnlockCommandError>;
    fn revoke_device(&mut self, device_id: &str) -> Result<UnlockSnapshot, UnlockCommandError>;
    fn open_unlock_diagnostics(&mut self) -> Result<UnlockDiagnostics, UnlockCommandError>;
}

#[derive(Default)]
pub struct GateClosedBackend;

impl UnlockBackend for GateClosedBackend {
    fn unlock_status(&mut self) -> Result<UnlockSnapshot, UnlockCommandError> {
        Ok(closed_unlock_snapshot())
    }

    fn begin_pairing(&mut self) -> Result<UnlockSnapshot, UnlockCommandError> {
        Err(UnlockCommandError::new(UnlockErrorCode::BackendUnavailable))
    }

    fn confirm_pairing(&mut self, _session_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        Err(UnlockCommandError::new(UnlockErrorCode::BackendUnavailable))
    }

    fn begin_calibration(
        &mut self,
        _device_id: &str,
    ) -> Result<UnlockSnapshot, UnlockCommandError> {
        Err(UnlockCommandError::new(UnlockErrorCode::BackendUnavailable))
    }

    fn revoke_device(&mut self, _device_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        Err(UnlockCommandError::new(UnlockErrorCode::BackendUnavailable))
    }

    fn open_unlock_diagnostics(&mut self) -> Result<UnlockDiagnostics, UnlockCommandError> {
        Ok(closed_unlock_diagnostics())
    }
}

pub struct UnlockCommandService<B> {
    backend: Mutex<B>,
}

impl<B> UnlockCommandService<B> {
    pub const fn new(backend: B) -> Self {
        Self {
            backend: Mutex::new(backend),
        }
    }
}

impl UnlockCommandService<GateClosedBackend> {
    pub fn closed() -> Self {
        Self::new(GateClosedBackend)
    }
}

impl<B: UnlockBackend> UnlockCommandService<B> {
    fn dispatch<T>(
        &self,
        caller: &str,
        operation: impl FnOnce(&mut B) -> Result<T, UnlockCommandError>,
    ) -> Result<T, UnlockCommandError> {
        ensure_main_window(caller)?;
        let mut backend = self
            .backend
            .try_lock()
            .map_err(|_| UnlockCommandError::new(UnlockErrorCode::Busy))?;
        operation(&mut backend)
    }

    pub fn unlock_status(&self, caller: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.dispatch(caller, UnlockBackend::unlock_status)
    }

    pub fn begin_pairing(&self, caller: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.dispatch(caller, UnlockBackend::begin_pairing)
    }

    pub fn confirm_pairing(
        &self,
        caller: &str,
        session_id: &str,
    ) -> Result<UnlockSnapshot, UnlockCommandError> {
        ensure_main_window(caller)?;
        validate_identifier(session_id)?;
        self.dispatch(caller, |backend| backend.confirm_pairing(session_id))
    }

    pub fn begin_calibration(
        &self,
        caller: &str,
        device_id: &str,
    ) -> Result<UnlockSnapshot, UnlockCommandError> {
        ensure_main_window(caller)?;
        validate_identifier(device_id)?;
        self.dispatch(caller, |backend| backend.begin_calibration(device_id))
    }

    pub fn revoke_device(
        &self,
        caller: &str,
        device_id: &str,
    ) -> Result<UnlockSnapshot, UnlockCommandError> {
        ensure_main_window(caller)?;
        validate_identifier(device_id)?;
        self.dispatch(caller, |backend| backend.revoke_device(device_id))
    }

    pub fn open_unlock_diagnostics(
        &self,
        caller: &str,
    ) -> Result<UnlockDiagnostics, UnlockCommandError> {
        self.dispatch(caller, UnlockBackend::open_unlock_diagnostics)
    }
}

fn ensure_main_window(caller: &str) -> Result<(), UnlockCommandError> {
    if caller == "main" {
        Ok(())
    } else {
        Err(UnlockCommandError::new(UnlockErrorCode::CallerNotAllowed))
    }
}

fn validate_identifier(value: &str) -> Result<(), UnlockCommandError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(UnlockCommandError::new(UnlockErrorCode::InvalidRequest))
    }
}

pub type ProductionUnlockCommandService =
    UnlockCommandService<crate::build_unlock_backend::BuildUnlockBackend>;

impl UnlockCommandService<crate::build_unlock_backend::BuildUnlockBackend> {
    pub fn for_current_build() -> Self {
        Self::new(crate::build_unlock_backend::BuildUnlockBackend::for_current_build())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PairingConfirmation {
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeviceSelection {
    device_id: String,
}

#[tauri::command]
pub(crate) fn unlock_status(
    window: WebviewWindow,
    service: State<'_, Arc<ProductionUnlockCommandService>>,
) -> Result<UnlockSnapshot, UnlockCommandError> {
    service.unlock_status(window.label())
}

#[tauri::command]
pub(crate) fn begin_pairing(
    window: WebviewWindow,
    service: State<'_, Arc<ProductionUnlockCommandService>>,
) -> Result<UnlockSnapshot, UnlockCommandError> {
    service.begin_pairing(window.label())
}

#[tauri::command]
pub(crate) fn confirm_pairing(
    window: WebviewWindow,
    service: State<'_, Arc<ProductionUnlockCommandService>>,
    value: PairingConfirmation,
) -> Result<UnlockSnapshot, UnlockCommandError> {
    service.confirm_pairing(window.label(), &value.session_id)
}

#[tauri::command]
pub(crate) fn begin_calibration(
    window: WebviewWindow,
    service: State<'_, Arc<ProductionUnlockCommandService>>,
    value: DeviceSelection,
) -> Result<UnlockSnapshot, UnlockCommandError> {
    service.begin_calibration(window.label(), &value.device_id)
}

#[tauri::command]
pub(crate) fn revoke_device(
    window: WebviewWindow,
    service: State<'_, Arc<ProductionUnlockCommandService>>,
    value: DeviceSelection,
) -> Result<UnlockSnapshot, UnlockCommandError> {
    service.revoke_device(window.label(), &value.device_id)
}

#[tauri::command]
pub(crate) fn open_unlock_diagnostics(
    window: WebviewWindow,
    service: State<'_, Arc<ProductionUnlockCommandService>>,
) -> Result<UnlockDiagnostics, UnlockCommandError> {
    service.open_unlock_diagnostics(window.label())
}
