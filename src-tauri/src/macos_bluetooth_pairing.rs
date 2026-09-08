use crate::debug_bluetooth_pairing::{
    BluetoothPairingPeer, BluetoothPairingPort, BluetoothPairingPortError,
    BluetoothPairingRadioState,
};
use std::ffi::{CStr, CString, c_char};

unsafe extern "C" {
    fn repose_ble_pairing_radio_state() -> i32;
    fn repose_ble_pairing_start(session: *const c_char) -> bool;
    fn repose_ble_pairing_stop();
    fn repose_ble_pairing_has_peer() -> bool;
    fn repose_ble_pairing_copy_peer_session(buffer: *mut c_char, capacity: usize) -> usize;
    fn repose_ble_pairing_copy_peer_identifier(buffer: *mut c_char, capacity: usize) -> usize;
    fn repose_ble_pairing_copy_peer_name(buffer: *mut c_char, capacity: usize) -> usize;
}

#[derive(Default)]
pub struct MacBluetoothPairingPort;

impl BluetoothPairingPort for MacBluetoothPairingPort {
    fn radio_state(&self) -> BluetoothPairingRadioState {
        // SAFETY: the Objective-C boundary returns only a small integer snapshot and owns
        // all CoreBluetooth objects on the main queue.
        match unsafe { repose_ble_pairing_radio_state() } {
            0 | 1 => BluetoothPairingRadioState::Ready,
            2 => BluetoothPairingRadioState::PoweredOff,
            3 => BluetoothPairingRadioState::Unauthorized,
            4 => BluetoothPairingRadioState::Unsupported,
            _ => BluetoothPairingRadioState::Failed,
        }
    }

    fn start_advertising(&mut self, session_id: &str) -> Result<(), BluetoothPairingPortError> {
        let session =
            CString::new(session_id).map_err(|_| BluetoothPairingPortError::InvalidSessionId)?;
        // SAFETY: CString guarantees a live NUL-terminated pointer for the duration of
        // the call; the native layer copies the value before returning.
        unsafe { repose_ble_pairing_start(session.as_ptr()) }
            .then_some(())
            .ok_or(BluetoothPairingPortError::AdvertisingRejected)
    }

    fn stop_advertising(&mut self) {
        // SAFETY: the native function has no arguments and synchronizes its CoreBluetooth
        // state onto the main queue.
        unsafe { repose_ble_pairing_stop() };
    }

    fn connected_peer(&self) -> Option<BluetoothPairingPeer> {
        // SAFETY: the native function returns a synchronized boolean snapshot.
        if !unsafe { repose_ble_pairing_has_peer() } {
            return None;
        }
        Some(BluetoothPairingPeer {
            session_id: copy_native_string(repose_ble_pairing_copy_peer_session)?,
            device_id: copy_native_string(repose_ble_pairing_copy_peer_identifier)?,
            display_name: copy_native_string(repose_ble_pairing_copy_peer_name)?,
        })
    }
}

fn copy_native_string(copy: unsafe extern "C" fn(*mut c_char, usize) -> usize) -> Option<String> {
    let mut buffer = [0 as c_char; 256];
    // SAFETY: `buffer` is writable for the supplied capacity. The native boundary always
    // NUL-terminates when capacity is non-zero and reports the untruncated byte length.
    let required = unsafe { copy(buffer.as_mut_ptr(), buffer.len()) };
    if required == 0 || required >= buffer.len() {
        return None;
    }
    // SAFETY: the native copy contract above guarantees a NUL terminator within `buffer`.
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_str()
        .ok()
        .map(str::to_owned)
}
