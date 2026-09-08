#![forbid(unsafe_code)]

use crate::unlock::{
    GateClosedBackend, UnlockBackend, UnlockCommandError, UnlockDiagnostics, UnlockSnapshot,
};

#[cfg(all(debug_assertions, target_os = "macos"))]
use crate::{
    debug_bluetooth_pairing::DebugBluetoothPairingBackend,
    debug_pairing_material::OsPairingMaterialSource,
    macos_bluetooth_pairing::MacBluetoothPairingPort,
};

pub enum BuildUnlockBackend {
    Closed(GateClosedBackend),
    #[cfg(all(debug_assertions, target_os = "macos"))]
    DebugBluetooth(
        Box<DebugBluetoothPairingBackend<MacBluetoothPairingPort, OsPairingMaterialSource>>,
    ),
}

impl BuildUnlockBackend {
    pub fn for_current_build() -> Self {
        #[cfg(all(debug_assertions, target_os = "macos"))]
        if let Ok(material_source) = OsPairingMaterialSource::new("Repose Mac") {
            return Self::DebugBluetooth(Box::new(DebugBluetoothPairingBackend::new(
                MacBluetoothPairingPort,
                material_source,
                epoch_millis,
            )));
        }
        Self::Closed(GateClosedBackend)
    }
}

impl UnlockBackend for BuildUnlockBackend {
    fn unlock_status(&mut self) -> Result<UnlockSnapshot, UnlockCommandError> {
        match self {
            Self::Closed(backend) => backend.unlock_status(),
            #[cfg(all(debug_assertions, target_os = "macos"))]
            Self::DebugBluetooth(backend) => backend.unlock_status(),
        }
    }

    fn begin_pairing(&mut self) -> Result<UnlockSnapshot, UnlockCommandError> {
        match self {
            Self::Closed(backend) => backend.begin_pairing(),
            #[cfg(all(debug_assertions, target_os = "macos"))]
            Self::DebugBluetooth(backend) => backend.begin_pairing(),
        }
    }

    fn confirm_pairing(&mut self, session_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        match self {
            Self::Closed(backend) => backend.confirm_pairing(session_id),
            #[cfg(all(debug_assertions, target_os = "macos"))]
            Self::DebugBluetooth(backend) => backend.confirm_pairing(session_id),
        }
    }

    fn begin_calibration(&mut self, device_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        match self {
            Self::Closed(backend) => backend.begin_calibration(device_id),
            #[cfg(all(debug_assertions, target_os = "macos"))]
            Self::DebugBluetooth(backend) => backend.begin_calibration(device_id),
        }
    }

    fn revoke_device(&mut self, device_id: &str) -> Result<UnlockSnapshot, UnlockCommandError> {
        match self {
            Self::Closed(backend) => backend.revoke_device(device_id),
            #[cfg(all(debug_assertions, target_os = "macos"))]
            Self::DebugBluetooth(backend) => backend.revoke_device(device_id),
        }
    }

    fn open_unlock_diagnostics(&mut self) -> Result<UnlockDiagnostics, UnlockCommandError> {
        match self {
            Self::Closed(backend) => backend.open_unlock_diagnostics(),
            #[cfg(all(debug_assertions, target_os = "macos"))]
            Self::DebugBluetooth(backend) => backend.open_unlock_diagnostics(),
        }
    }
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn epoch_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
