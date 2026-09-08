#![forbid(unsafe_code)]

use crate::unlock::{
    CalibrationPhase, CompanionPlatform, ComponentHealth, PairedDevice, PairingSession,
    UnlockBackend, UnlockCapability, UnlockCommandError, UnlockDiagnostics, UnlockErrorCode,
    UnlockSnapshot, closed_unlock_diagnostics, closed_unlock_snapshot,
};
use std::{error::Error, fmt};

pub const PAIRING_PENDING_CANDIDATE_NAME: &str = "等待手机连接";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BluetoothPairingRadioState {
    #[default]
    Unknown,
    Ready,
    PoweredOff,
    Unauthorized,
    Unsupported,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BluetoothPairingPeer {
    pub session_id: String,
    pub device_id: String,
    pub display_name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BluetoothPairingPortError {
    InvalidSessionId,
    AdvertisingRejected,
}

impl fmt::Display for BluetoothPairingPortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSessionId => formatter.write_str("invalid Bluetooth pairing session ID"),
            Self::AdvertisingRejected => {
                formatter.write_str("Bluetooth pairing advertisement was rejected")
            }
        }
    }
}

impl Error for BluetoothPairingPortError {}

pub trait BluetoothPairingPort: Send {
    fn radio_state(&self) -> BluetoothPairingRadioState;
    fn start_advertising(&mut self, session_id: &str) -> Result<(), BluetoothPairingPortError>;
    fn stop_advertising(&mut self);
    fn connected_peer(&self) -> Option<BluetoothPairingPeer>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingMaterial {
    pub session_id: String,
    pub qr_payload: String,
    pub expires_at_epoch_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairingMaterialError {
    InvalidMacName,
    ExpiryOverflow,
    InvalidPayload,
}

impl fmt::Display for PairingMaterialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMacName => formatter.write_str("invalid Mac pairing display name"),
            Self::ExpiryOverflow => formatter.write_str("pairing expiry timestamp overflowed"),
            Self::InvalidPayload => formatter.write_str("pairing payload generation failed"),
        }
    }
}

impl Error for PairingMaterialError {}

pub trait PairingMaterialSource: Send {
    fn create(&mut self, now_epoch_ms: u64) -> Result<PairingMaterial, PairingMaterialError>;
}

pub struct DebugBluetoothPairingBackend<P, M> {
    port: P,
    material_source: M,
    clock: Box<dyn Fn() -> u64 + Send>,
    pending: Option<PairingMaterial>,
    paired_devices: Vec<PairedDevice>,
}

impl<P, M> DebugBluetoothPairingBackend<P, M>
where
    P: BluetoothPairingPort,
    M: PairingMaterialSource,
{
    pub fn new(port: P, material_source: M, clock: impl Fn() -> u64 + Send + 'static) -> Self {
        Self {
            port,
            material_source,
            clock: Box::new(clock),
            pending: None,
            paired_devices: Vec::new(),
        }
    }

    pub fn set_test_clock(&mut self, clock: impl Fn() -> u64 + Send + 'static) {
        self.clock = Box::new(clock);
    }

    fn now(&self) -> u64 {
        (self.clock)()
    }

    fn clear_if_expired(&mut self) -> bool {
        let expired = self
            .pending
            .as_ref()
            .is_some_and(|pending| self.now() >= pending.expires_at_epoch_ms);
        if expired {
            self.port.stop_advertising();
            self.pending = None;
        }
        expired
    }

    fn clear_if_transport_unavailable(&mut self) -> bool {
        let unavailable = self.pending.is_some() && !self.transport_is_ready();
        if unavailable {
            self.port.stop_advertising();
            self.pending = None;
        }
        unavailable
    }

    fn transport_is_ready(&self) -> bool {
        self.port.radio_state() == BluetoothPairingRadioState::Ready
    }

    fn snapshot(&self) -> UnlockSnapshot {
        let mut snapshot = closed_unlock_snapshot();
        snapshot.components.service = ComponentHealth::Ready;
        snapshot.components.transport = if self.transport_is_ready() {
            ComponentHealth::Ready
        } else {
            ComponentHealth::Unavailable
        };
        snapshot.capability = if self.transport_is_ready() {
            UnlockCapability::Ready
        } else {
            UnlockCapability::ServiceUnavailable
        };
        snapshot.devices = self.paired_devices.clone();
        snapshot.calibration.phase = if snapshot.devices.is_empty() {
            CalibrationPhase::Unavailable
        } else {
            CalibrationPhase::Idle
        };
        snapshot.pending_pairing = self.pending.as_ref().map(|pending| {
            let candidate_name = self
                .matching_peer(pending)
                .map(|peer| peer.display_name)
                .unwrap_or_else(|| PAIRING_PENDING_CANDIDATE_NAME.to_owned());
            PairingSession {
                session_id: pending.session_id.clone(),
                candidate_name,
                qr_payload: pending.qr_payload.clone(),
                expires_at_epoch_ms: pending.expires_at_epoch_ms,
            }
        });
        snapshot
    }

    fn matching_peer(&self, pending: &PairingMaterial) -> Option<BluetoothPairingPeer> {
        let peer = self.port.connected_peer()?;
        if peer.session_id != pending.session_id
            || !valid_identifier(&peer.device_id)
            || !valid_display_name(&peer.display_name)
        {
            return None;
        }
        Some(peer)
    }
}

impl<P, M> UnlockBackend for DebugBluetoothPairingBackend<P, M>
where
    P: BluetoothPairingPort,
    M: PairingMaterialSource,
{
    fn unlock_status(&mut self) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.clear_if_expired();
        self.clear_if_transport_unavailable();
        Ok(self.snapshot())
    }

    fn begin_pairing(&mut self) -> Result<UnlockSnapshot, UnlockCommandError> {
        self.clear_if_expired();
        self.clear_if_transport_unavailable();
        if !self.transport_is_ready() {
            return Err(command_error(UnlockErrorCode::BackendUnavailable));
        }
        if self.pending.is_some() {
            return Err(command_error(UnlockErrorCode::Busy));
        }
        let material = self
            .material_source
            .create(self.now())
            .map_err(|_| command_error(UnlockErrorCode::BackendUnavailable))?;
        if !valid_identifier(&material.session_id)
            || material.qr_payload.is_empty()
            || material.qr_payload.len() > 4096
            || material.expires_at_epoch_ms <= self.now()
        {
            return Err(command_error(UnlockErrorCode::BackendUnavailable));
        }
        self.port
            .start_advertising(&material.session_id)
            .map_err(|_| command_error(UnlockErrorCode::BackendUnavailable))?;
        self.pending = Some(material);
        Ok(self.snapshot())
    }

    fn confirm_pairing(&mut self, session_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        if self.clear_if_expired() {
            return Err(command_error(UnlockErrorCode::PairingExpired));
        }
        if self.clear_if_transport_unavailable() {
            return Err(command_error(UnlockErrorCode::BackendUnavailable));
        }
        let pending = self
            .pending
            .as_ref()
            .filter(|pending| pending.session_id == session_id)
            .ok_or_else(|| command_error(UnlockErrorCode::BackendUnavailable))?;
        let peer = self
            .matching_peer(pending)
            .ok_or_else(|| command_error(UnlockErrorCode::BackendUnavailable))?;
        self.paired_devices
            .retain(|device| device.id != peer.device_id);
        self.paired_devices.push(PairedDevice {
            id: peer.device_id,
            display_name: peer.display_name,
            platform: CompanionPlatform::Android,
        });
        self.pending = None;
        self.port.stop_advertising();
        Ok(self.snapshot())
    }

    fn begin_calibration(
        &mut self,
        _device_id: &str,
    ) -> Result<UnlockSnapshot, UnlockCommandError> {
        Err(command_error(UnlockErrorCode::BackendUnavailable))
    }

    fn revoke_device(&mut self, device_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        let before = self.paired_devices.len();
        self.paired_devices.retain(|device| device.id != device_id);
        if self.paired_devices.len() == before {
            return Err(command_error(UnlockErrorCode::DeviceNotFound));
        }
        Ok(self.snapshot())
    }

    fn open_unlock_diagnostics(&mut self) -> Result<UnlockDiagnostics, UnlockCommandError> {
        Ok(closed_unlock_diagnostics())
    }
}

fn command_error(code: UnlockErrorCode) -> UnlockCommandError {
    UnlockCommandError { code }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_display_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 80 && value.chars().all(|character| !character.is_control())
}
