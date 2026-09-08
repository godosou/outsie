//! BLE adapter and in-memory authorization from the existing pairing flow.
use crate::console_ble_wire::BleCipher;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use repose_unlock_core::pairing::decode_pairing_payload;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::Duration,
};
use zeroize::Zeroizing;

struct Credential {
    device_id: String,
    name: String,
    secret: Zeroizing<[u8; 32]>,
}
fn credentials() -> &'static Mutex<HashMap<[u8; 16], Credential>> {
    static DATA: OnceLock<Mutex<HashMap<[u8; 16], Credential>>> = OnceLock::new();
    DATA.get_or_init(Default::default)
}
static LINK_GENERATION: AtomicU64 = AtomicU64::new(1);
static RADIO_READY: AtomicBool = AtomicBool::new(false);
pub fn link_generation() -> u64 {
    LINK_GENERATION.load(Ordering::SeqCst)
}
pub fn bluetooth_ready() -> bool {
    RADIO_READY.load(Ordering::Relaxed)
}
#[derive(Clone, Serialize)]
pub struct PairedConsoleDevice {
    pub id: String,
    pub name: String,
}
pub fn paired_devices() -> Vec<PairedConsoleDevice> {
    let mut devices: Vec<_> = credentials()
        .lock()
        .unwrap()
        .values()
        .map(|d| PairedConsoleDevice {
            id: d.device_id.clone(),
            name: d.name.clone(),
        })
        .collect();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices
}
pub fn authorize_pairing(qr: &str, device_id: &str, name: &str) -> Result<(), String> {
    let raw = qr
        .strip_prefix("repose://pair/v1/")
        .ok_or("Invalid pairing payload")?;
    let raw = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| "Invalid pairing payload")?;
    let payload = decode_pairing_payload(&raw).map_err(|_| "Invalid pairing payload")?;
    authorize(
        *payload.session_id(),
        *payload.pairing_secret(),
        device_id,
        name,
    );
    Ok(())
}
fn authorize(session: [u8; 16], secret: [u8; 32], device_id: &str, name: &str) {
    let mut data = credentials().lock().unwrap();
    data.retain(|_, d| d.device_id != device_id);
    data.insert(
        session,
        Credential {
            device_id: device_id.into(),
            name: name.into(),
            secret: Zeroizing::new(secret),
        },
    );
    LINK_GENERATION.fetch_add(1, Ordering::SeqCst);
}
pub fn revoke_device(device_id: &str) {
    credentials()
        .lock()
        .unwrap()
        .retain(|_, d| d.device_id != device_id);
    LINK_GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(sender) = sender().lock().unwrap().as_ref() {
        let _ = sender.try_send(Event {
            central: String::new(),
            kind: 5,
            data: vec![],
            generation: link_generation(),
        });
    }
}
/// Only debug simulation can inject a known test credential. Never exposed to Tauri.
#[cfg(debug_assertions)]
pub fn authorize_simulation() {
    authorize([1; 16], [2; 32], "simulated-phone", "仿真手机");
}

type Handler = dyn Fn(Value, u64) -> Result<Value, String> + Send + Sync;
type LinkReset = dyn Fn() + Send + Sync;
pub struct BleSession {
    challenge: [u8; 16],
    cipher: Option<BleCipher>,
    session: Option<[u8; 16]>,
}
impl BleSession {
    pub fn new(challenge: [u8; 16]) -> Self {
        Self {
            challenge,
            cipher: None,
            session: None,
        }
    }
    pub fn process(
        &mut self,
        packet: &[u8],
        handler: &Handler,
        generation: u64,
    ) -> Result<Vec<u8>, String> {
        let session = BleCipher::session(packet)?;
        if packet.len() > 16 * 1024 + 58 {
            return Err("BLE request too large".into());
        }
        let data = credentials().lock().unwrap();
        let credential = data
            .get(&session)
            .ok_or("Phone is not paired on this Mac")?;
        if self.session.is_some_and(|s| s != session) {
            return Err("BLE peer changed".into());
        }
        let cipher = self.cipher.get_or_insert_with(|| {
            BleCipher::new(&credential.secret, session, self.challenge, true)
        });
        let plain = cipher.decrypt(packet)?;
        drop(data);
        self.session = Some(session);
        let request: Value = serde_json::from_slice(&plain).map_err(|_| "Invalid control JSON")?;
        if generation != link_generation() {
            return Err("BLE connection changed".into());
        }
        let response = match handler(request, generation) {
            Ok(data) => json!({"ok":true,"data":data}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        let output = serde_json::to_vec(&response).map_err(|_| "Invalid control response")?;
        cipher.encrypt(&output)
    }
}
struct Event {
    central: String,
    kind: i32,
    data: Vec<u8>,
    generation: u64,
}
fn sender() -> &'static Mutex<Option<SyncSender<Event>>> {
    static SENDER: OnceLock<Mutex<Option<SyncSender<Event>>>> = OnceLock::new();
    SENDER.get_or_init(Default::default)
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn repose_ble_console_start(
        callback: extern "C" fn(*const std::ffi::c_char, i32, *const u8, usize),
    ) -> bool;
    fn repose_ble_console_stop();
    fn repose_ble_console_revoke(
        central: *const std::ffi::c_char,
        challenge: *const u8,
        len: usize,
    ) -> bool;
    fn repose_ble_console_send(
        central: *const std::ffi::c_char,
        data: *const u8,
        len: usize,
    ) -> bool;
}
#[cfg(target_os = "macos")]
extern "C" fn native_event(
    central: *const std::ffi::c_char,
    kind: i32,
    data: *const u8,
    len: usize,
) {
    if kind == 4 {
        if !data.is_null() && len == 1 {
            RADIO_READY.store(unsafe { *data } == 1, Ordering::Relaxed);
        }
        return;
    }
    if len > 256 * 1024 || (len > 0 && data.is_null()) {
        return;
    }
    if kind == 1 || kind == 3 {
        LINK_GENERATION.fetch_add(1, Ordering::SeqCst);
    }
    let name = if central.is_null() {
        String::new()
    } else {
        unsafe { std::ffi::CStr::from_ptr(central) }
            .to_string_lossy()
            .into_owned()
    };
    let bytes = if len == 0 {
        vec![]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }.to_vec()
    };
    if let Some(tx) = sender().lock().unwrap().as_ref()
        && tx
            .try_send(Event {
                central: name,
                kind,
                data: bytes,
                generation: link_generation(),
            })
            .is_err()
    {
        LINK_GENERATION.fetch_add(1, Ordering::SeqCst);
    }
}
pub struct BluetoothTransport {
    active: Arc<AtomicBool>,
}
impl BluetoothTransport {
    pub fn start(handler: Arc<Handler>, reset: Arc<LinkReset>) -> Result<Self, String> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (handler, reset);
            Err("蓝牙控制仅支持 Mac".into())
        }
        #[cfg(target_os = "macos")]
        {
            let (tx, rx) = mpsc::sync_channel::<Event>(16);
            *sender().lock().unwrap() = Some(tx);
            let active = Arc::new(AtomicBool::new(true));
            let alive = active.clone();
            thread::Builder::new()
                .name("console-bluetooth".into())
                .spawn(move || {
                    let mut peer: Option<(String, BleSession)> = None;
                    while alive.load(Ordering::Acquire) {
                        let event = match rx.recv_timeout(Duration::from_millis(100)) {
                            Ok(e) => e,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(_) => break,
                        };
                        if !alive.load(Ordering::Acquire) {
                            break;
                        }
                        match event.kind {
                            1 if event.data.len() == 16 => {
                                reset();
                                peer = Some((
                                    event.central,
                                    BleSession::new(event.data.try_into().unwrap()),
                                ));
                            }
                            3 | 5 => {
                                reset();
                                if let Some((central, session)) = peer.take()
                                    && event.kind == 5
                                {
                                    revoke_native(&central, &session.challenge);
                                }
                            }
                            2 => {
                                let Some((central, session)) = peer.as_mut() else {
                                    continue;
                                };
                                if *central != event.central
                                    || event.generation != link_generation()
                                {
                                    continue;
                                }
                                let result = session.process(
                                    &event.data,
                                    handler.as_ref(),
                                    event.generation,
                                );
                                if !alive.load(Ordering::Acquire)
                                    || event.generation != link_generation()
                                {
                                    continue;
                                }
                                match result {
                                    Ok(response) => {
                                        if !send_native(central, &response) {
                                            reset();
                                            peer = None;
                                        }
                                    }
                                    Err(_) => {
                                        reset();
                                        revoke_native(central, &session.challenge);
                                        peer = None;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                })
                .map_err(|_| "无法启动蓝牙控制")?;
            if !unsafe { repose_ble_console_start(native_event) } {
                active.store(false, Ordering::Release);
                *sender().lock().unwrap() = None;
                return Err("蓝牙不可用，请检查系统蓝牙权限".into());
            }
            Ok(Self { active })
        }
    }
    pub fn stop(&self) {
        if !self.active.swap(false, Ordering::AcqRel) {
            return;
        }
        LINK_GENERATION.fetch_add(1, Ordering::SeqCst);
        #[cfg(target_os = "macos")]
        unsafe {
            repose_ble_console_stop();
        }
        *sender().lock().unwrap() = None;
    }
}
#[cfg(target_os = "macos")]
fn revoke_native(central: &str, challenge: &[u8; 16]) -> bool {
    let Ok(central) = std::ffi::CString::new(central) else {
        return false;
    };
    unsafe { repose_ble_console_revoke(central.as_ptr(), challenge.as_ptr(), challenge.len()) }
}
#[cfg(target_os = "macos")]
fn send_native(central: &str, data: &[u8]) -> bool {
    let Ok(central) = std::ffi::CString::new(central) else {
        return false;
    };
    unsafe { repose_ble_console_send(central.as_ptr(), data.as_ptr(), data.len()) }
}
impl Drop for BluetoothTransport {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unpaired_and_revoked_keys_cannot_control() {
        let secret = [44; 32];
        let session = [45; 16];
        let challenge = [46; 16];
        let mut phone = BleCipher::new(&secret, session, challenge, false);
        let packet = phone.encrypt(b"{}").unwrap();
        let mut mac = BleSession::new(challenge);
        let handler = |_: Value, _: u64| Ok(json!({"accepted":true}));
        assert!(mac.process(&packet, &handler, link_generation()).is_err());
        authorize(session, secret, "wire-test", "test");
        assert!(mac.process(&packet, &handler, link_generation()).is_ok());
        revoke_device("wire-test");
        assert!(
            mac.process(&phone.encrypt(b"{}").unwrap(), &handler, link_generation())
                .is_err()
        );
    }
}
