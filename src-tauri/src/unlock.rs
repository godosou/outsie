//! Phone Key (手机钥匙) backend — the Rust half of the §6 contract in
//! docs/plans/2026-09-09-unlock-interaction-design.md. Replaces the old
//! GateClosedBackend placeholder: shells into the already-verified scripts
//! (native/macos/minimal-auth-plugin/{install,uninstall,healthcheck}.sh,
//! tools/ble-spike/mac/{rssi-scan,permit-bridge.sh}) instead of reimplementing
//! them. `std::process::Command` from a Tauri process needs no entitlement.
//!
//! The JSON shapes here are the exact mirror of src/lib/unlock.ts: structs are
//! camelCase; UnlockErrorCode/UnlockState/Presence/Health/ComponentId are
//! kebab-case strings; Remediation and ComponentInvocation are internally tagged
//! on `kind` (so `{ "kind": "fix-on-phone", "hint": "unseen" }` etc.), matching
//! the TypeScript discriminated unions.
//!
//! Tonight's scope is simulation: this compiles, its unit tests pass, and the
//! read-only snapshot works; the privileged `osascript … with administrator
//! privileges` install/uninstall paths are written correctly but only exercised
//! on a real Mac tomorrow.

#![allow(dead_code)] // several commands are wired for tomorrow's real-Mac pass

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tauri::{AppHandle, Manager};

// ---- §6.1 error contract -------------------------------------------------

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum UnlockErrorCode {
    PreflightRuleShape,
    AuthorizationDenied,
    InstallFailed,
    HalfInstalled,
    ComponentNotLoaded,
    RuleTampered,
    BackupMissing,
    PairingExpired,
    PairingMismatch,
    CalibrationOverlap,
    KeyMismatch,
    BluetoothOff,
    BluetoothUnauthorized,
    PhoneAppStopped,
    PhoneUnseen,
    DaemonUnavailable,
    DrillNotObserved,
    Unsupported,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum PhoneHint {
    BluetoothOff,
    AppStopped,
    Battery,
    Unseen,
}

// Internally tagged on `kind` to match the TS `{ kind: 'reinstall-component' }`
// discriminated union.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Remediation {
    ReinstallComponent,
    RepairRule,
    RePair,
    ReCalibrate,
    FixOnPhone { hint: PhoneHint },
    RevokeDevice,
    UninstallAndRestore,
    LeaveItAlone,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub expected: String,
    pub actual: String,
    pub read_at: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UnlockError {
    pub code: UnlockErrorCode,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<Remediation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Evidence>,
}

impl UnlockError {
    fn new(code: UnlockErrorCode, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into(), remediation: None, evidence: None }
    }
    fn with_remediation(mut self, r: Remediation) -> Self {
        self.remediation = Some(r);
        self
    }
}

// ---- §6.3 snapshot -------------------------------------------------------

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum UnlockState {
    NotInstalled,
    Installing,
    HalfInstalled,
    AwaitingPasswordDrill,
    AwaitingPairing,
    AwaitingCalibration,
    AwaitingVerification,
    Ready,
    NeedsRepair,
    Paused,
    Uninstalling,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Presence {
    Near,
    Away,
    TransportUnavailable,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub enum RuleVariant {
    A,
    B,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentId {
    Rule,
    Component,
    Daemon,
    Transport,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Health {
    Ok,
    Degraded,
    Broken,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockComponent {
    pub id: ComponentId,
    pub health: Health,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Evidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<Remediation>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ComponentInvocation {
    Observed { at: String },
    NeverObserved,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    /// The key slot it occupies, chosen by the phone at pairing.
    pub id: String,
    pub name: String,
    pub platform: String,
    /// ISO time the key file was written. Empty when unreadable -- the card
    /// then omits the line rather than showing a made-up date.
    pub paired_at: String,
    /// True only when `.provenance` records a real SAS pairing. A key pushed
    /// over USB by a dev script can unlock this Mac too, so it belongs in the
    /// list; calling it a paired phone is what would be false.
    pub paired: bool,
    /// False for a key installed before pairing recorded which phone it came
    /// from. Such a key still works; what the Mac cannot do is recognise the
    /// phone again, so re-pairing leaves this one behind instead of replacing
    /// it. Guessing that it belongs to whoever pairs next would silently delete
    /// a second phone's key on a Mac that has two.
    pub identified: bool,
    /// Whether this device can unlock right now, and why not when it cannot.
    pub can_unlock: bool,
    /// Whether the OWNER has allowed this phone to unlock, separately from
    /// whether anything is watching. Two different questions: the first is a
    /// decision, the second is a state.
    pub unlock_allowed: bool,
    /// Whether this phone may press shortcuts. Off by default -- pairing is
    /// consent to unlock, not consent to type.
    pub control_allowed: bool,
    pub blocked_reason: Option<String>,
}

/// Split the pairing tool's one line of stdout: `<keyId> <phoneIdHex> <keyHex>`.
///
/// Pure, and strict about every field. A malformed line has to fail the pairing
/// rather than be half-read: a key id that silently became 0, or a key that
/// silently became empty, would install something the verifier refuses while
/// the panel reported 配对完成 -- which is precisely how the printf bug got as
/// far as a lock screen.
pub fn parse_pair_output(line: &str) -> Option<(u8, String, String, String)> {
    let mut parts = line.trim().split_whitespace();
    let key_id: u8 = parts.next()?.parse().ok()?;
    if key_id == 0 {
        return None;
    }
    let phone_id = parts.next()?.to_ascii_lowercase();
    if phone_id.len() != 16 || !phone_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let hex32 = |p: Option<&str>| -> Option<String> {
        let v = p?.to_string();
        (v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit())).then_some(v)
    };
    let key = hex32(parts.next())?;
    // The catalogue key. Required, not optional: without it the phone cannot
    // check a button list, and a button list it cannot check is one that can
    // mislabel every button on it.
    let console_key = hex32(parts.next())?;
    // Anything after that is not something this version understands, and
    // guessing at it is how a format change becomes a silent misinstall.
    if parts.next().is_some() {
        return None;
    }
    Some((key_id, phone_id, key, console_key))
}

/// The shell that deletes one device's key. Pure so the quoting and the fact
/// that it takes the sidecars with it are testable: a key deleted while its
/// provenance stays behind leaves the next key looking SAS-paired when it was
/// pushed over USB, and a `.phone` left behind (seen on 2026-09-12: slot 166's
/// phone id outlived its key by hours) keeps naming a phone this Mac no longer
/// has a key for.
pub fn revoke_script(key_path: &str) -> String {
    format!(
        "do shell script \"rm -f {key} {key}.provenance {key}.phone\" with administrator privileges",
        key = applescript_quote(key_path),
    )
}

/// One key file on this Mac, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct KeySlot {
    /// The slot number in `presence-key.<id>`. Chosen by the phone from v3 on;
    /// every key written before that is in slot 1.
    pub id: u8,
    /// Whether `.phone` records which phone this key came from.
    pub identified: bool,
    pub state: PresenceKeyState,
    /// ISO time the file was written, i.e. when pairing finished.
    pub written_at: Option<String>,
}

/// What the panel's health model should say about a Mac holding these slots.
///
/// A Mac with two phones is healthy when either key is usable; it is in trouble
/// when one is misowned, because that one will be refused by the verifier while
/// the panel goes on saying 已配对. So a single bad slot wins over any number of
/// good ones -- the bad one is the news.
pub fn aggregate_key_state(slots: &[KeySlot]) -> PresenceKeyState {
    if let Some(bad) = slots.iter().find(|s| matches!(s.state, PresenceKeyState::BadPermissions { .. })) {
        return bad.state.clone();
    }
    match slots.iter().find(|s| matches!(s.state, PresenceKeyState::Ok { .. })) {
        // `paired` is true only if EVERY usable key came from a real pairing.
        // One dev key among them and the panel must not vouch for the set.
        Some(_) => PresenceKeyState::Ok {
            paired: slots
                .iter()
                .filter(|s| matches!(s.state, PresenceKeyState::Ok { .. }))
                .all(|s| matches!(s.state, PresenceKeyState::Ok { paired: true })),
        },
        None => PresenceKeyState::Missing,
    }
}

/// Every phone that can open this Mac, one row each.
pub fn paired_devices(
    slots: &[KeySlot],
    name_for: &dyn Fn(u8) -> Option<String>,
    watching: bool,
    caps: &DeviceCapabilities,
) -> Vec<PairedDevice> {
    let mut out: Vec<PairedDevice> = slots
        .iter()
        .filter_map(|slot| {
            let paired = match &slot.state {
                PresenceKeyState::Missing | PresenceKeyState::BadPermissions { .. } => return None,
                PresenceKeyState::Ok { paired } => *paired,
            };
            Some(device_row(
                slot.id,
                paired,
                slot.identified,
                name_for(slot.id),
                slot.written_at.clone(),
                watching,
                caps,
            ))
        })
        .collect();
    // Stable order, so a list of phones does not reshuffle between two reads of
    // a directory.
    out.sort_by_key(|d| d.id.parse::<u8>().unwrap_or(0));
    out
}

fn device_row(
    id: u8,
    paired: bool,
    identified: bool,
    saved_name: Option<String>,
    paired_at: Option<String>,
    watching: bool,
    caps: &DeviceCapabilities,
) -> PairedDevice {
    let name = match (paired, saved_name.as_deref().map(str::trim).filter(|n| !n.is_empty())) {
        (true, Some(n)) => n.to_string(),
        (true, None) => "已配对的手机".to_string(),
        (false, _) => "USB 下发的开发密钥".to_string(),
    };
    let allowed = caps.unlock_allowed(id);
    PairedDevice {
        id: id.to_string(),
        name,
        platform: if paired { "Android" } else { "开发用" }.to_string(),
        paired_at: paired_at.unwrap_or_default(),
        paired,
        identified,
        can_unlock: watching && allowed,
        unlock_allowed: allowed,
        control_allowed: caps.control_allowed(id),
        // The reason names whichever switch is actually off, and the phone's own
        // one first: if both are off, telling someone to go flip the master is
        // sending them to the wrong place.
        blocked_reason: match (allowed, watching) {
            (false, _) => Some("这部手机的解锁开关关着".to_string()),
            (true, false) => Some("上面的总开关关着，现在谁都解不了锁".to_string()),
            (true, true) => None,
        },
    }
}

/// What each phone is allowed to do, as the owner decided.
///
/// Separate from whether it CAN: a phone may be allowed to unlock while nothing
/// is watching for it. Collapsing the two would make the switch read as broken
/// whenever the monitor happened to be off.
#[derive(Clone, Debug, Default)]
pub struct DeviceCapabilities {
    /// Slots explicitly switched off for unlocking. Absence means allowed:
    /// pairing IS consent to unlock, and a phone that had to be enabled after
    /// pairing would look like a pairing that had not finished.
    pub unlock_off: Vec<u8>,
    /// Slots explicitly allowed to press shortcuts. Absence means NOT allowed,
    /// the opposite default: pairing is consent to unlock, not consent to type
    /// into whatever is open.
    pub control_on: Vec<u8>,
}

impl DeviceCapabilities {
    pub fn unlock_allowed(&self, id: u8) -> bool {
        !self.unlock_off.contains(&id)
    }
    pub fn control_allowed(&self, id: u8) -> bool {
        self.control_on.contains(&id)
    }
}

pub const CAPABILITIES_FILE: &str = "device-capabilities.json";

/// What each phone on this Mac is allowed to do.
pub fn load_capabilities(app: &AppHandle) -> DeviceCapabilities {
    app.path()
        .app_data_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join(CAPABILITIES_FILE)).ok())
        .map(|raw| parse_capabilities(&raw))
        .unwrap_or_default()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityArgs {
    pub device_id: String,
    /// "unlock" or "control".
    pub capability: String,
    pub allowed: bool,
}

/// Flip one phone's permission.
///
/// No administrator prompt, and that is not a shortcut: neither switch can grant
/// anything. Turning unlock OFF makes this Mac stricter, and turning control ON
/// still leaves every command subject to the paired key and the accessibility
/// grant. A file the user can write cannot loosen either.
#[tauri::command]
pub fn unlock_set_device_capability(
    app: AppHandle,
    value: CapabilityArgs,
) -> Result<UnlockSnapshot, UnlockError> {
    let id: u8 = value.device_id.parse().map_err(|_| {
        UnlockError::new(UnlockErrorCode::Unsupported, format!("不是一个钥匙编号：{}", value.device_id))
    })?;
    let mut caps = load_capabilities(&app);
    match value.capability.as_str() {
        "unlock" => {
            caps.unlock_off.retain(|x| *x != id);
            if !value.allowed {
                caps.unlock_off.push(id);
            }
        }
        "control" => {
            caps.control_on.retain(|x| *x != id);
            if value.allowed {
                caps.control_on.push(id);
            }
        }
        other => {
            return Err(UnlockError::new(
                UnlockErrorCode::Unsupported,
                format!("不认识的权限「{other}」"),
            ))
        }
    }
    caps.unlock_off.sort_unstable();
    caps.control_on.sort_unstable();

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?;
    let _ = std::fs::create_dir_all(&dir);
    let body = serde_json::json!({ "unlockOff": caps.unlock_off, "controlOn": caps.control_on });
    let path = dir.join(CAPABILITIES_FILE);
    let tmp = path.with_extension("json.writing");
    std::fs::write(&tmp, body.to_string())
        .and_then(|_| std::fs::rename(&tmp, &path))
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, format!("存不下来：{e}")))?;

    // The bridge reads the disabled list itself, so unlock takes effect on the
    // next beacon rather than on the next pipeline restart -- a switch that
    // needed an administrator password to take effect is a switch nobody flips.
    let _ = write_disabled_keys(&app, &caps);
    HostMacBackend::new(&app).get_snapshot()
}

/// The list the bridge reads, one id per line.
///
/// In the pipeline's work directory, which the user can write. That is safe in
/// exactly one direction: every id in this file makes the Mac refuse a phone it
/// would otherwise accept. Nothing here can grant access, so a file anyone can
/// edit cannot be used to gain any.
fn write_disabled_keys(app: &AppHandle, caps: &DeviceCapabilities) -> std::io::Result<()> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .join("presence-run");
    let _ = std::fs::create_dir_all(&dir);
    let body: String = caps.unlock_off.iter().map(|id| format!("{id}\n")).collect();
    std::fs::write(dir.join("disabled-keys"), body)
}

/// Parse the stored capabilities. Anything unreadable means the defaults, which
/// are "unlock yes, control no" -- the safe direction for both.
pub fn parse_capabilities(raw: &str) -> DeviceCapabilities {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return DeviceCapabilities::default();
    };
    let ids = |key: &str| -> Vec<u8> {
        v.get(key)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|n| n.as_u64()).filter_map(|n| u8::try_from(n).ok()).collect())
            .unwrap_or_default()
    };
    DeviceCapabilities { unlock_off: ids("unlockOff"), control_on: ids("controlOn") }
}


#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockStats {
    pub unlocks_today: u32,
    pub last_unlock_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSnapshot {
    pub read_at: String,
    pub state: UnlockState,
    pub presence: Presence,
    /// Whether anything is watching for the phone. This is what the panel's
    /// switch shows and what it changes -- see [presence_running].
    pub presence_running: bool,
    pub variant: Option<RuleVariant>,
    pub components: Vec<UnlockComponent>,
    pub component_invocation: ComponentInvocation,
    /// Every phone that can open this Mac. Was a single Option, which was
    /// correct only while one key slot existed.
    pub devices: Vec<PairedDevice>,
    pub stats: UnlockStats,
    pub last_failure: Option<serde_json::Value>,
    pub macos_build: String,
    pub component_version: String,
    /// Four hex digits identifying this Mac in the beacon the phone hears.
    /// None until the monitor has published at least one window.
    pub mac_id: Option<String>,
    /// What the radio is doing, separately from what the phone is doing.
    pub radio: RadioState,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub variant: Option<RuleVariant>,
    pub can_install: bool,
    pub rule_now: String,
    /// Third-party mechanisms already in the lock-screen rule. Empty on an
    /// untouched Mac. Not a blocker -- a disclosure: with k-of-n = 1 each of
    /// these can already grant an unlock on its own, and adding ours makes one
    /// more. The install sheet must show these before anyone agrees.
    pub foreign: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReport {
    pub read_at: String,
    pub rule_now: String,
    pub backup_used: bool,
    pub diff_against_backup: Vec<String>,
    pub right_removed: bool,
    pub bundle_removed: bool,
    pub keys_removed: bool,
    pub residual: Vec<String>,
}

/// Where a live `repose-pair-v2` exchange has got to.
///
/// This replaced a struct holding `code: "4F2K9A"` and
/// `qr_payload: "repose-pair://placeholder"` -- a fixed string the panel
/// displayed as though it were a pairing code, next to a QR payload that
/// pointed nowhere. Meanwhile the only working pairing lived in a shell script
/// the user had to find themselves, and the phone's own screen told them to
/// press a button in this app that did not exist.
///
/// The six digits here are the real SAS. They are read from the pairing tool,
/// never invented, and `Compare` is the one stage where a human decision is
/// load-bearing: it is the whole of the man-in-the-middle defence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairingStage {
    /// Nothing running.
    Idle,
    /// The tool is up and looking for a phone in pairing mode.
    Scanning,
    /// Digits are on screen; waiting for the human to say whether they match.
    Compare,
    /// This Mac has its key and is waiting to hear the phone use it.
    ///
    /// Both ends must be told by a human that the digits matched -- see the
    /// note on Compare -- so confirming here cannot finish the job. It used to
    /// say 完成 anyway, which is how someone ends up with a Mac that is paired
    /// and a phone that is not, and no screen anywhere saying so.
    ///
    /// The Mac can find out, though: once the phone has its key it starts
    /// signing beacons with it, and a beacon that verifies here is proof both
    /// ends hold the same key. That is a fact, not a relayed claim, so it is
    /// safe to wait on.
    WaitingForPhone,
    /// Key derived and written to this Mac.
    Done,
    /// Over, without a key. `detail` says why in plain language.
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSession {
    pub stage: PairingStage,
    /// The six digits, only in `Compare`.
    pub digits: Option<String>,
    /// The paired key's short fingerprint, only in `Done`.
    ///
    /// No longer the headline. Two opaque codes in one flow -- six digits to
    /// compare and eight hex characters to ignore -- left people asking which
    /// one mattered, and that is the worst question to be unsure about here.
    /// It is still computed and still comparable; it now sits behind 技术细节.
    pub fingerprint: Option<String>,
    /// What the phone calls itself. Cosmetic: not covered by the SAS
    /// transcript, so it identifies nobody — it is there because a human can
    /// hold "realme GT" in their head and cannot hold "E6A0704D".
    pub peer_name: Option<String>,
    /// Plain-language explanation, mainly for `Failed`.
    pub detail: Option<String>,
}

impl PairingSession {
    fn stage(stage: PairingStage) -> Self {
        Self { stage, digits: None, fingerprint: None, peer_name: None, detail: None }
    }
    fn failed(detail: impl Into<String>) -> Self {
        Self {
            stage: PairingStage::Failed,
            digits: None,
            fingerprint: None,
            peer_name: None,
            detail: Some(detail.into()),
        }
    }
}

/// Turn the pairing tool's exit code into something a person can act on.
///
/// Code 5 is the one that matters: the phone revealed a nonce that does not
/// match the commitment it published earlier, which is what a man in the middle
/// leaves behind. It must never be worded as a glitch worth retrying.
pub fn pairing_failure(code: Option<i32>) -> String {
    match code {
        Some(2) => format!("这台 Mac 的蓝牙用不了。检查蓝牙是否打开、系统设置里是否允许 {BRAND} 使用蓝牙，然后再试一次。"),
        Some(3) => "没找到正在配对的手机。在手机上点「开始配对」，把手机放在 Mac 旁边，再试一次。".into(),
        Some(4) => "配对过程中断了。重新配一次即可。".into(),
        Some(5) => "已中止：手机后来公布的信息和它先前的承诺对不上。\
                    这正是有人在中间冒充会留下的痕迹。换个地方、离开可疑的环境，再重新配对。"
            .into(),
        Some(6) => "已中止。没有写入任何密钥。".into(),
        Some(7) => "等太久了，这次配对已经作废。重新开始即可。".into(),
        _ => "配对没有完成，没有写入任何密钥。".into(),
    }
}

/// What the panel should be showing, given what is on disk and whether the tool
/// is still alive.
///
/// Pure so the stage machine can be tested without a radio or a phone: the one
/// transition that must never happen by accident is reaching `Done` without a
/// human having seen `Compare`.
pub fn pairing_stage(digits: Option<&str>, exit_code: Option<Option<i32>>) -> PairingSession {
    match (digits, exit_code) {
        // Exited before the digits ever appeared: nothing was compared.
        (None, Some(code)) => PairingSession::failed(pairing_failure(code)),
        (None, None) => PairingSession::stage(PairingStage::Scanning),
        // Digits were shown, but the tool is gone -- the window closed before
        // the human answered. Keeping `Compare` on screen would leave a live
        // pair of buttons in front of a process that cannot receive the answer,
        // so the honest report is that this attempt is over.
        (Some(_), Some(code)) => PairingSession::failed(pairing_failure(code)),
        (Some(d), None) => PairingSession {
            stage: PairingStage::Compare,
            digits: Some(d.trim().to_string()),
            fingerprint: None,
            peer_name: None,
            detail: None,
        },
    }
}


// ---- command argument shapes ---------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallArgs {
    pub variant: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairArgs {
    pub target: String,
}
#[derive(Deserialize)]
pub struct EnabledArgs {
    pub enabled: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceArgs {
    pub device_id: String,
}
#[derive(Deserialize)]
pub struct DrillArgs {
    pub kind: String,
}
#[derive(Deserialize)]
pub struct PauseArgs {
    pub minutes: u32,
}

// ---- pure helpers (unit-tested) ------------------------------------------

/// Decide the rule form from the screensaver rule's `rule` array (the JSON that
/// `security authorizationdb read … | plutil -extract rule json` produces) and
/// the mechanism/subrule name we install. Form A leaves `use-login-window-ui`;
/// form B swaps it for `authenticate-session-owner-or-admin`. Absent our entry,
/// A is the default target. Returns None when the rule is not a shape we edit.
pub fn decide_variant(rule_json: &str) -> Option<RuleVariant> {
    let arr: Vec<String> = serde_json::from_str(rule_json).ok()?;
    if arr.iter().any(|e| e == "authenticate-session-owner-or-admin") {
        Some(RuleVariant::B)
    } else if arr.iter().any(|e| e == "use-login-window-ui") {
        Some(RuleVariant::A)
    } else {
        None
    }
}

/// Entries in the screensaver rule that are neither Apple's nor ours.
///
/// Found on a real machine: this Mac's rule already read
///
///   ["com.openai.sky.CUAService.AuthorizationPlugin.remote", "use-login-window-ui"]
///
/// with k-of-n = 1, installed by a computer-use agent. decide_variant said
/// Some(A) -- it keys on the Apple mechanism being present, not on the rule being
/// untouched -- so preflight would have approved, and the installer would have
/// added a third entry to a lock screen a stranger had already reconfigured.
///
/// k-of-n = 1 means ANY one entry can grant the unlock. Stacking onto that is not
/// automatically wrong, but it is a decision about someone's lock screen, and it
/// was being made for them silently. This names what is already there so the
/// install sheet can say it out loud.
///
/// The list is of things we recognise, not of things we distrust: an unknown
/// entry is unknown, which is the whole point. `ai.repose.spike` counts as known
/// because finding ourselves means a previous install, not a stranger.
pub fn foreign_mechanisms(rule_json: &str) -> Vec<String> {
    const KNOWN: [&str; 5] = [
        "use-login-window-ui",
        "authenticate-session-owner-or-admin",
        "authenticate-session-owner",
        "authenticate",
        SUBRULE_NAME,
    ];
    serde_json::from_str::<Vec<String>>(rule_json)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !KNOWN.contains(&e.as_str()))
        .collect()
}

fn variant_str(v: Option<RuleVariant>) -> Option<&'static str> {
    match v {
        Some(RuleVariant::A) => Some("A"),
        Some(RuleVariant::B) => Some("B"),
        None => None,
    }
}

// ---- what the host actually looks like ------------------------------------

/// Paths and labels the installer writes. Duplicated from install.sh, which is
/// the source of truth; `unlock_installer_paths_agree` asserts they match, so a
/// rename there fails a test here instead of silently making this panel report
/// "not installed" on a machine that is installed.
pub const BUNDLE_PATH: &str = "/Library/Security/SecurityAgentPlugins/ReposeSpike.bundle";
pub const DAEMON_LABEL: &str = "ai.repose.spike.healthcheck";
pub const SUBRULE_NAME: &str = "ai.repose.spike";
pub const PRESENCE_KEY_DIR: &str = "/var/db/repose-unlock";

/// The product's name, for the strings this module shows people.
///
/// brand.json at the repo root is the canonical record; this is the Rust
/// declaration site, and src/lib/brandAssets.test.ts asserts the two agree.
/// Deliberately separate from PRESENCE_KEY_DIR and the identifiers around it:
/// those are on disk and on the air and must survive a rename untouched.
pub const BRAND: &str = "Outsie";

/// Where permit-bridge.sh publishes its decision, one line: `state,rssi,unix_s`.
/// User-owned by design -- it is a display signal, not an authorization input.
/// Nothing here can grant an unlock; the permit the plugin reads is root-only
/// and written separately.
pub const STATUS_FILE: &str = "presence-status";

/// How stale a published line may be before it stops meaning anything.
///
/// The bridge re-publishes on every refresh (every REPOSE_REFRESH_S, 5s by
/// default), so a line older than this means the bridge is not running --
/// which is NOT the same as the phone being away, and must not render as it.
const STATUS_MAX_AGE_S: i64 = 30;

/// One reading of the bridge's status file.
#[derive(Clone, Debug, PartialEq)]
pub enum PresenceReport {
    /// No file: the bridge has never run here.
    NeverRan,
    /// A line older than [STATUS_MAX_AGE_S], or one saying the bridge stopped.
    NotRunning,
    /// Running, and this is its verdict.
    Fresh { state: String, rssi: Option<i32> },
    /// A file we could not parse. Treated as "not running" for the state
    /// machine, kept distinct so the panel can say why.
    Unreadable,
    /// Scanning, but nothing is verifying: the administrator prompt that starts
    /// the privileged half was not completed.
    NoAuthorization,
}

/// Turn a published line and its age into what the panel shows.
///
/// The distinction that matters: "your phone is away" and "the thing that
/// watches for your phone is not running" both mean the password path, but only
/// one of them means the feature is working. Collapsing them shows a calm,
/// correct-looking Away state for a Mac where nothing is watching at all.
pub fn read_presence(line: Option<&str>, now_s: i64) -> PresenceReport {
    let Some(line) = line else { return PresenceReport::NeverRan };
    let f: Vec<&str> = line.trim().split(',').collect();
    if f.len() < 3 {
        return PresenceReport::Unreadable;
    }
    let Ok(stamp) = f[2].parse::<i64>() else { return PresenceReport::Unreadable };
    // A stamp from the future is a clock that moved, not a fresher reading.
    if stamp > now_s + STATUS_MAX_AGE_S || now_s - stamp > STATUS_MAX_AGE_S {
        return PresenceReport::NotRunning;
    }
    match f[0] {
        "stopped" => PresenceReport::NotRunning,
        // The scanner came up and the privileged half did not, because the
        // authorization prompt was never completed. Distinct from "not running":
        // something IS running, it just cannot verify anything, and the panel
        // must not let that read as either working or merely stopped.
        "noauth" => PresenceReport::NoAuthorization,
        state => PresenceReport::Fresh {
            state: state.to_string(),
            rssi: f[1].parse::<i32>().ok(),
        },
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PresenceKeyState {
    /// No key file. Nothing can authenticate, so nothing can unlock.
    Missing,
    /// A key is installed. `paired` records HOW it got here: true when this app
    /// ran `repose-pair-v2` and a human compared six digits, false when it was
    /// pushed over USB by a development script.
    ///
    /// The difference is the entire man-in-the-middle defence, so the panel is
    /// not allowed to describe them with one sentence. It used to: every
    /// provisioned key was labelled "通过 USB 下发的开发密钥，不是带防中间人校验
    /// 的配对", which became false the moment real pairing shipped -- the same
    /// shape of stale claim, just pointing the other way.
    Ok { paired: bool },
    /// Present but not root:wheel 0600 — a key someone else could rewrite is a
    /// key someone else could become the paired phone with.
    BadPermissions { detail: String },
}

/// One reading of the host, gathered by [HostMacBackend::observe]. Kept separate
/// from [assess] so the interesting states -- especially the fail-open one, which
/// is hard to produce on a real machine and dangerous to leave lying around --
/// can be unit tested without a Mac in that condition.
#[derive(Clone, Debug)]
pub struct HostFacts {
    pub rule_references_us: bool,
    pub bundle_present: bool,
    /// None when codesign could not be asked at all.
    pub bundle_signature_ok: Option<bool>,
    /// None when launchctl could not be asked at all.
    pub daemon_loaded: Option<bool>,
    pub presence_key: PresenceKeyState,
    pub presence: PresenceReport,
}

/// Is anything actually watching for the phone right now?
///
/// This is what the panel's switch means, and the switch has to be derived from
/// something it can change. It used to be derived from `state`, which it could
/// not: an installed, paired Mac reports AwaitingVerification whether or not
/// the monitor is running, so the switch rendered ON permanently, only ever ran
/// its turn-OFF branch, and the turn-ON branch was unreachable. Clicking it did
/// nothing visible, forever.
///
/// NoAuthorization counts as running on purpose. Something IS running -- the
/// scanner -- and calling that "off" would offer the user a switch to turn on
/// a thing that is already on. The transport row is where the breakage gets
/// explained.
pub fn presence_running(report: &PresenceReport) -> bool {
    match report {
        PresenceReport::Fresh { .. } | PresenceReport::NoAuthorization => true,
        PresenceReport::NeverRan | PresenceReport::NotRunning | PresenceReport::Unreadable => false,
    }
}

pub struct Assessment {
    pub state: UnlockState,
    pub presence: Presence,
    pub presence_running: bool,
    pub components: Vec<UnlockComponent>,
}

fn component(
    id: ComponentId,
    health: Health,
    detail: &str,
    remediation: Option<Remediation>,
) -> UnlockComponent {
    UnlockComponent { id, health, detail: detail.into(), evidence: None, remediation }
}

/// Turn a reading of the host into what the panel shows.
///
/// The case this exists for is the third one below: the lock-screen rule points
/// at our mechanism and the mechanism is not there. E3/E8/E11 established that
/// macOS treats an un-instantiable step as *passed*, so that machine unlocks for
/// anyone with no password, and no rule shape can prevent it. It is also the
/// state a user reaches by dragging the app to the Trash. A panel that renders
/// it as "组件缺失" among a list of tidy grey rows would be describing an open
/// door as a missing accessory, so it gets `Broken`, `NeedsRepair`, and a
/// sentence that says what is true right now.
pub fn assess(f: &HostFacts) -> Assessment {
    let mut components = Vec::new();

    // --- rule + component, judged together ---------------------------------
    let loadable = f.bundle_present && f.bundle_signature_ok != Some(false);
    let dangling = f.rule_references_us && !loadable;

    if f.rule_references_us {
        components.push(component(
            ComponentId::Rule,
            if dangling { Health::Broken } else { Health::Ok },
            if dangling { "指向一个装不上的组件" } else { "已就位" },
            dangling.then_some(Remediation::UninstallAndRestore),
        ));
    }

    if dangling {
        let detail = if !f.bundle_present {
            "组件不在了，但锁屏规则还指着它。macOS 会把装不上的这一步当作已通过 —— \
             也就是说，现在任何人都可能不用密码就进得来。请立即修复或卸载。"
        } else {
            "组件在，但签名校验不通过，macOS 不会加载它。规则仍指着它，\
             等于现在这台 Mac 可能不用密码就进得来。请立即修复或卸载。"
        };
        components.push(component(
            ComponentId::Component,
            Health::Broken,
            detail,
            Some(Remediation::ReinstallComponent),
        ));
    } else if f.bundle_present && !f.rule_references_us {
        components.push(component(
            ComponentId::Component,
            Health::Degraded,
            "组件已安装，但锁屏规则没有引用它 —— 解锁不会发生，密码照常能登录。",
            Some(Remediation::RepairRule),
        ));
    } else if f.bundle_present {
        components.push(component(
            ComponentId::Component,
            match f.bundle_signature_ok {
                Some(true) => Health::Ok,
                // Unaskable is not the same as bad, and must not read as fine.
                None => Health::Unknown,
                Some(false) => Health::Broken,
            },
            match f.bundle_signature_ok {
                Some(true) => "已安装，签名校验通过",
                None => "已安装；这次没能校验签名",
                Some(false) => "签名校验不通过",
            },
            None,
        ));
    }

    // --- the health-check daemon -------------------------------------------
    // Only meaningful once the rule references us: before that there is nothing
    // for it to guard, and listing it as broken would be noise.
    if f.rule_references_us {
        components.push(match f.daemon_loaded {
            Some(true) => component(ComponentId::Daemon, Health::Ok, "运行中", None),
            Some(false) => component(
                ComponentId::Daemon,
                Health::Degraded,
                "没有运行。它的职责是在组件被删掉时把规则改回只认密码；\
                 它不在，那个窗口就没人盯着了。",
                Some(Remediation::ReinstallComponent),
            ),
            None => component(ComponentId::Daemon, Health::Unknown, "这次没问出来", None),
        });
    }

    // --- transport: the key AND whether anything is watching -----------------
    //
    // ONE row, not two. The first version pushed a second Transport component
    // when the bridge was not running, so the panel listed 配对密钥 twice saying
    // different things and the reader had to guess which one counted.
    //
    // Both facts share a row because they answer the same user question -- "can
    // my phone let me in right now" -- and the row shows whichever answer is
    // worse. A key problem outranks a bridge problem: without a key nothing can
    // ever be recognised, whereas a stopped bridge is a thing you restart.
    let transport = match (&f.presence_key, &f.presence) {
        (PresenceKeyState::BadPermissions { detail }, _) => {
            component(ComponentId::Transport, Health::Broken, detail, Some(Remediation::RePair))
        }
        (PresenceKeyState::Missing, _) => component(
            ComponentId::Transport,
            Health::Degraded,
            "还没有和手机配对，所以不会自动解锁。密码照常能登录。点上面的「配一部新手机」。",
            Some(Remediation::RePair),
        ),
        // A key exists; now, is anything actually watching? "Away" is the
        // ordinary state of a phone in another room and must not look like a
        // fault, but "nobody is watching" must not look like Away.
        (PresenceKeyState::Ok { .. }, PresenceReport::NeverRan) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测还没有运行过。手机钥匙不会生效，密码照常能登录。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok { .. }, PresenceReport::NotRunning) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测没有在运行 —— 这不是「手机不在」，是没人在看。密码照常能登录。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok { .. }, PresenceReport::NoAuthorization) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测没有拿到管理员授权，所以只有扫描在跑，没有任何东西在验证 —— \
             手机钥匙不会生效，密码照常能登录。把开关关掉再打开，这次在密码框里完成授权。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok { .. }, PresenceReport::Unreadable) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测的状态读不出来，当作没有在运行处理。密码照常能登录。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok { paired }, PresenceReport::Fresh { state, .. }) => component(
            ComponentId::Transport,
            Health::Ok,
            match (paired, state.as_str()) {
                (true, "near") => "已和手机配对，监测运行中，手机在附近。",
                (true, _) => "已和手机配对，监测运行中，现在没看到手机。",
                // Not a nag: this key defends against nobody in the middle, and
                // the row that says "一切正常" is the only place someone would
                // find that out.
                (false, "near") => "监测运行中，手机在附近。注意：这把密钥是通过 USB 下发的开发密钥，\
                                    没有经过两端核对数字的配对，挡不住中间人。重新配对一次会换成真的。",
                (false, _) => "监测运行中，现在没看到手机。注意：这把密钥是通过 USB 下发的开发密钥，\
                               没有经过两端核对数字的配对，挡不住中间人。重新配对一次会换成真的。",
            },
            if *paired { None } else { Some(Remediation::RePair) },
        ),
    };
    components.push(transport);

    // Presence comes from the bridge's published line, never from a guess. Three
    // of the four reports mean transport-unavailable; the row above is where the
    // difference between them is spelled out.
    let presence = match &f.presence {
        PresenceReport::Fresh { state, .. } if state == "near" => Presence::Near,
        PresenceReport::Fresh { state, .. } if state == "away" => Presence::Away,
        _ => Presence::TransportUnavailable,
    };

    // --- overall ------------------------------------------------------------
    let state = if dangling {
        UnlockState::NeedsRepair
    } else if f.rule_references_us && f.bundle_present {
        // Installed and consistent. Not `Ready`: nothing here has watched the
        // mechanism actually run.
        //
        // Without a key, pairing comes first. This used to go straight to
        // AwaitingVerification, whose button is 「锁屏，试一次」 -- so a freshly
        // installed Mac invited you to lock the screen and watch nothing
        // happen, because no phone could possibly authenticate yet. Worse, the
        // state that offers 配对手机 existed in the enum and was never once
        // returned, which is why the only route to pairing was a shell script.
        if matches!(f.presence_key, PresenceKeyState::Missing) {
            UnlockState::AwaitingPairing
        } else {
            UnlockState::AwaitingVerification
        }
    } else if f.rule_references_us || f.bundle_present {
        UnlockState::HalfInstalled
    } else {
        UnlockState::NotInstalled
    };

    Assessment { state, presence, presence_running: presence_running(&f.presence), components }
}

// ---- backend trait + host implementation ---------------------------------

/// Injectable so unit tests can drive a fake. The real implementation shells to
/// the verified scripts on the host Mac.
pub trait UnlockBackend {
    fn get_snapshot(&self) -> Result<UnlockSnapshot, UnlockError>;
    fn preflight(&self) -> Result<PreflightReport, UnlockError>;
    fn install(&self, variant: Option<RuleVariant>) -> Result<UnlockSnapshot, UnlockError>;
    fn repair(&self, target: &str) -> Result<UnlockSnapshot, UnlockError>;
    fn uninstall(&self) -> Result<UninstallReport, UnlockError>;
    fn set_enabled(&self, enabled: bool) -> Result<UnlockSnapshot, UnlockError>;
    fn revoke_device(&self, device_id: &str) -> Result<UnlockSnapshot, UnlockError>;
}

pub struct HostMacBackend {
    /// Directory holding install.sh / uninstall.sh / healthcheck.sh / authdb-edit,
    /// resolved from the bundle's resources (or a dev fallback).
    scripts_dir: Option<PathBuf>,
    /// Kept so the read-only snapshot can find the app's data dir, where the
    /// presence bridge publishes. Cheap to clone; it is a handle, not the app.
    app: AppHandle,
}

impl HostMacBackend {
    pub fn new(app: &AppHandle) -> Self {
        Self { scripts_dir: resolve_scripts_dir(app), app: app.clone() }
    }

    fn now_iso() -> String {
        // Avoid pulling chrono; use `date -u` for an ISO-8601 stamp. Falls back
        // to an empty string only if `date` is somehow unavailable.
        run_capture("/bin/date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_default()
    }

    /// Read the host once. Every probe is read-only and none needs root: the key
    /// file's directory is traversable, so its ownership and mode can be read
    /// without reading the key -- which is the point, this process has no
    /// business holding it.
    /// Read the bridge's published line, if there is one.
    fn presence_report(app: &AppHandle) -> PresenceReport {
        // Not a security input: this file only decides what the panel says. The
        // permit the plugin actually reads is root-only and written elsewhere.
        let now = run_capture("/bin/date", &["+%s"])
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        let Some(path) = status_path(app) else { return PresenceReport::NeverRan };
        match std::fs::read_to_string(&path) {
            Ok(text) => read_presence(Some(&text), now),
            Err(_) => PresenceReport::NeverRan,
        }
    }

    fn observe(app: &AppHandle, rule: Option<&str>) -> HostFacts {
        let bundle_present = std::path::Path::new(BUNDLE_PATH).exists();
        HostFacts {
            rule_references_us: rule.is_some_and(|r| r.contains(SUBRULE_NAME)),
            bundle_present,
            // Failing to *run* codesign is not the same as codesign saying no.
            // Collapsing them would report a healthy Mac as wide open, and a
            // false alarm about the one state that really matters is how that
            // alarm stops being read.
            bundle_signature_ok: bundle_present
                .then(|| run_status("/usr/bin/codesign", &["--verify", "--deep", BUNDLE_PATH]).ok())
                .flatten(),
            daemon_loaded: Some(
                run_status("/bin/launchctl", &["print", &format!("system/{DAEMON_LABEL}")])
                    .unwrap_or(false),
            ),
            presence_key: aggregate_key_state(&Self::key_slots()),
            presence: Self::presence_report(app),
        }
    }

    /// The nickname the phone reported during pairing, saved next to the
    /// pairing state. Cosmetic, and absent on a Mac paired before it was
    /// recorded -- both are fine; the card falls back to a generic label.
    fn saved_peer_name(app: &AppHandle, id: u8) -> Option<String> {
        let dir = app.path().app_data_dir().ok()?.join("pairing");
        // Per-slot first; `peer-name.saved` is where every pairing before v3
        // put it, and that one belongs to slot 1. Reading it for any other slot
        // would label a second phone with the first one's name.
        std::fs::read_to_string(dir.join(format!("peer-name.{id}.saved")))
            .ok()
            .or_else(|| (id == 1).then(|| std::fs::read_to_string(dir.join("peer-name.saved")).ok()).flatten())
    }

    /// When the key file was written, which is when pairing finished.
    fn key_written_at(id: u8) -> Option<String> {
        let md = std::fs::metadata(format!("{PRESENCE_KEY_DIR}/presence-key.{id}")).ok()?;
        let secs = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        // Same idiom as now_iso: shell to `date` rather than pull a date crate
        // in for two call sites.
        run_capture("/bin/date", &["-u", "-r", &secs.to_string(), "+%Y-%m-%dT%H:%M:%SZ"])
    }

    /// Read the scanner's log, and how long the pipeline has been up.
    ///
    /// The run flag is created by the pipeline as the last thing before the
    /// privileged half starts, so its mtime is when this run began -- and it is
    /// already the file the whole pipeline hangs on, so nothing new has to be
    /// written to answer this.
    fn radio(app: &AppHandle, running: bool) -> RadioState {
        if !running {
            return RadioState::Starting;
        }
        let Ok(dir) = app.path().app_data_dir() else { return RadioState::Starting };
        let work = dir.join("presence-run");
        let up_for = std::fs::read_dir(&work)
            .ok()
            .and_then(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("running."))
                    .filter_map(|e| e.metadata().ok()?.modified().ok())
                    .max()
            })
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        radio_state(std::fs::read_to_string(work.join("scan.log")).ok().as_deref(), up_for)
    }

    /// Every `presence-key.<n>` on this Mac.
    ///
    /// A directory scan rather than a hardcoded slot 1: the phone chooses its
    /// own id from protocol v3 on, so the Mac cannot know in advance which
    /// files exist. Unreadable directory means no keys, which fails closed.
    fn key_slots() -> Vec<KeySlot> {
        let Ok(entries) = std::fs::read_dir(PRESENCE_KEY_DIR) else { return Vec::new() };
        let mut slots: Vec<KeySlot> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                // `.provenance` files live beside the keys and must not be read
                // as keys themselves.
                let id: u8 = name.strip_prefix("presence-key.")?.parse().ok()?;
                Some(KeySlot {
                    id,
                    identified: Self::slot_phone_id(id).is_some(),
                    state: Self::slot_state(id),
                    written_at: Self::key_written_at(id),
                })
            })
            .collect();
        slots.sort_by_key(|s| s.id);
        slots
    }

    /// Which phone a slot belongs to, as written beside the key at pairing.
    /// None for keys installed before v3, which is why a phone with no recorded
    /// identity never matches and never causes a replacement.
    fn slot_phone_id(id: u8) -> Option<String> {
        std::fs::read_to_string(format!("{PRESENCE_KEY_DIR}/presence-key.{id}.phone"))
            .ok()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit()))
    }

    fn slot_state(id: u8) -> PresenceKeyState {
        use std::os::unix::fs::MetadataExt;
        let path = format!("{PRESENCE_KEY_DIR}/presence-key.{id}");
        let Ok(md) = std::fs::metadata(&path) else {
            return PresenceKeyState::Missing;
        };
        let mode = md.mode() & 0o777;

        // Size, because the contents are unreadable from here.
        //
        // This checked permissions and nothing else, so a corrupt key passed:
        // a three-byte file, written by a printf bug, was reported as 已配对.
        // The panel then offered 「锁屏，试一次」 -- a drill that could only
        // fail -- and never offered 配对手机, because that button appears only
        // when the key is MISSING. The state had no way out through the UI.
        //
        // The file is root-only 0600 so its bytes cannot be read here, but the
        // directory is 755 and a key is 64 hex characters, with or without a
        // trailing newline. Any other length is not a key the verifier will
        // take, and calling it one is the same lie in a smaller place.
        let len = md.len();
        if len != 64 && len != 65 {
            return PresenceKeyState::Missing;
        }

        if md.uid() == 0 && md.gid() == 0 && mode == 0o600 {
            // How the key got here, recorded next to it by the pairing path.
            // Absent means it was pushed by a dev script -- and absent is the
            // safe reading: claiming a key was verified when we cannot tell
            // would be the one wrong direction to guess in.
            let paired = std::fs::read_to_string(format!("{path}.provenance"))
                .map(|s| {
                    let s = s.trim();
                    s == "repose-pair-v2" || s == "repose-pair-v3"
                })
                .unwrap_or(false);
            PresenceKeyState::Ok { paired }
        } else {
            PresenceKeyState::BadPermissions {
                detail: format!(
                    "在场密钥 {path} 应当是 root:wheel 0600，实际是 uid={} gid={} 权限={:o}。\
                     验证器会拒绝使用它 —— 一把别人能改写的密钥，等于别人能冒充你的手机。",
                    md.uid(),
                    md.gid(),
                    mode
                ),
            }
        }
    }

    fn read_rule() -> Option<String> {
        // security authorizationdb read … | plutil -extract rule json -o - -
        let raw = run_capture(
            "/usr/bin/security",
            &["authorizationdb", "read", "system.login.screensaver"],
        )?;
        run_capture_stdin("/usr/bin/plutil", &["-extract", "rule", "json", "-o", "-", "-"], &raw)
    }
}

impl UnlockBackend for HostMacBackend {
    fn get_snapshot(&self) -> Result<UnlockSnapshot, UnlockError> {
        // Read-only; never throws. Unreadable pieces become health=unknown / a
        // not-installed shape. componentInvocation stays never-observed unless a
        // daemon status file proves otherwise (not read tonight -> never).
        let read_at = Self::now_iso();
        let rule = Self::read_rule();
        let macos_build = run_capture("/usr/bin/sw_vers", &["-buildVersion"]).unwrap_or_default();

        let variant = rule.as_deref().and_then(decide_variant);
        let assessment = assess(&Self::observe(&self.app, rule.as_deref()));

        Ok(UnlockSnapshot {
            read_at,
            state: assessment.state,
            presence: assessment.presence,
            presence_running: assessment.presence_running,
            variant,
            components: assessment.components,
            component_invocation: ComponentInvocation::NeverObserved,
            devices: paired_devices(
                &Self::key_slots(),
                &|id| Self::saved_peer_name(&self.app, id),
                assessment.presence_running,
                &load_capabilities(&self.app),
            ),
            stats: UnlockStats { unlocks_today: 0, last_unlock_at: None },
            last_failure: None,
            macos_build,
            component_version: env!("CARGO_PKG_VERSION").to_string(),
            mac_id: mac_identity(&verified_csv(&self.app)),
            radio: Self::radio(&self.app, assessment.presence_running),
        })
    }

    fn preflight(&self) -> Result<PreflightReport, UnlockError> {
        let rule_now = Self::read_rule().unwrap_or_default();
        match decide_variant(&rule_now) {
            Some(variant) => {
                let foreign = foreign_mechanisms(&rule_now);
                Ok(PreflightReport { variant: Some(variant), can_install: true, rule_now, foreign })
            }
            None => Err(UnlockError::new(
                UnlockErrorCode::PreflightRuleShape,
                &format!("这台 Mac 的锁屏规则和预期不同，{BRAND} 不改它"),
            )),
        }
    }

    fn install(&self, variant: Option<RuleVariant>) -> Result<UnlockSnapshot, UnlockError> {
        let script = self.scripts_dir.as_ref()
            .map(|d| d.join("install.sh"))
            .filter(|p| p.exists())
            .ok_or_else(|| UnlockError::new(UnlockErrorCode::InstallFailed, "找不到安装脚本"))?;
        let _ = variant_str(variant); // form is chosen inside the script's preflight
        // ASSUME_YES, because there is nobody to answer the script's own prompt.
        // install.sh asks "Proceed? [y/N]" on a terminal; under `do shell script`
        // there is no stdin, so the read hit EOF and the installer aborted --
        // after the user had already agreed in the app's own disclosure and typed
        // their password. uninstall.sh was passed this from the start; install was
        // simply missed, and the generic failure message hid which one it was.
        let cmd = format!(
            "do shell script \"ASSUME_YES=1 {} permit\" with administrator privileges",
            shell_quote(&script.to_string_lossy()),
        );
        run_privileged(&cmd)?;
        self.get_snapshot()
    }

    fn repair(&self, target: &str) -> Result<UnlockSnapshot, UnlockError> {
        // For the spike, repair re-runs install (its authdb-edit add-subrule is
        // prepend-preserving, so it only re-adds our one entry).
        let _ = target;
        self.install(None)
    }

    fn uninstall(&self) -> Result<UninstallReport, UnlockError> {
        let script = self.scripts_dir.as_ref()
            .map(|d| d.join("uninstall.sh"))
            .filter(|p| p.exists())
            .ok_or_else(|| UnlockError::new(UnlockErrorCode::InstallFailed, "找不到卸载脚本"))?;
        let cmd = format!(
            "do shell script \"ASSUME_YES=1 {} \" with administrator privileges",
            shell_quote(&script.to_string_lossy()),
        );
        // Stop watching before removing what it watches for. Left running, the
        // scanner keeps writing permits into a machine with no plugin to read
        // them -- harmless, but it also leaves the panel reporting a live
        // monitor for a feature that has just been uninstalled.
        let _ = set_presence_running(&self.app, false);

        run_privileged(&cmd)?;

        // Read, do not assume. Every field below used to be a literal.
        let facts = RemovalFacts {
            rule_now: Self::read_rule().unwrap_or_default(),
            bundle_present: std::path::Path::new(BUNDLE_PATH).exists(),
            key_present: std::fs::metadata(
                format!("{PRESENCE_KEY_DIR}/presence-key.1"),
            )
            .is_ok(),
            permit_dir_present: std::path::Path::new("/var/run/repose-spike").exists(),
            support_dir_present: std::path::Path::new(
                "/Library/Application Support/ReposeSpike",
            )
            .exists(),
        };
        Ok(uninstall_report(Self::now_iso(), &facts))
    }

    fn set_enabled(&self, _enabled: bool) -> Result<UnlockSnapshot, UnlockError> {
        // Pause/resume toggles the BLE LaunchAgent (Step 7), not authorizationdb.
        // Wired tomorrow; today just re-read.
        self.get_snapshot()
    }

    /// Delete the pairing key for one device, leaving the plugin installed so
    /// another phone can be paired.
    ///
    /// This was a no-op that re-read the snapshot while the UI's confirm step
    /// told the user 「已撤销，这台 Mac 现在只接受密码」. It never rendered because
    /// `device` was hardcoded to None; the moment that field carried a real
    /// phone, the lie would have shipped. So it is implemented rather than
    /// removed -- revoking a key and uninstalling the whole feature are
    /// genuinely different things to want.
    fn revoke_device(&self, device_id: &str) -> Result<UnlockSnapshot, UnlockError> {
        // The id names a real slot, or nothing is deleted. Accepting anything
        // and deleting slot 1 was safe only while slot 1 was the only slot;
        // with two phones it would delete whichever one is not being revoked.
        let id: u8 = device_id.parse().map_err(|_| {
            UnlockError::new(UnlockErrorCode::Unsupported, format!("不是一个钥匙编号：{device_id}"))
        })?;
        if !Self::key_slots().iter().any(|s| s.id == id) {
            return Err(UnlockError::new(
                UnlockErrorCode::Unsupported,
                format!("这台 Mac 上没有编号 {id} 的钥匙"),
            ));
        }
        let key = format!("{PRESENCE_KEY_DIR}/presence-key.{id}");
        run_privileged(&revoke_script(&key))?;

        // Read back before reporting. The password prompt was the user's, and
        // what it bought them has to be checked, not assumed.
        if std::fs::metadata(&key).is_ok() {
            return Err(UnlockError::new(
                UnlockErrorCode::InstallFailed,
                format!("{key} 还在。这部手机仍然可以解锁这台 Mac。"),
            ));
        }
        // The nickname is only meaningful next to the key it named -- and only
        // THAT key's nickname: removing the shared pre-v3 file while revoking
        // slot 7 would strip slot 1's name too.
        if let Ok(dir) = self.app.path().app_data_dir() {
            let pairing = dir.join("pairing");
            let _ = std::fs::remove_file(pairing.join(format!("peer-name.{id}.saved")));
            // The catalogue-signing key too: it only ever labelled buttons
            // for the phone that just lost its presence key, and keeping it
            // would let a stale slot keep signing lists for nobody.
            let _ = std::fs::remove_file(pairing.join(format!("console-key.{id}")));
            if id == 1 {
                let _ = std::fs::remove_file(pairing.join("peer-name.saved"));
            }
        }
        // The verifier drops a key whose file is gone within a second on its
        // own (presence-verify re-checks the file on every cache hit), so the
        // running pipeline needs no restart -- and no second password prompt.
        self.get_snapshot()
    }
}


/// What the radio is doing, read from the scanner's own log.
///
/// The gap this closes: with Bluetooth not yet granted, macOS holds the scanner
/// at a permission dialog and CoreBluetooth never delivers a state -- so the
/// log stays completely empty, no sample is ever taken, and the panel went on
/// saying 「已开启 · 正在留意你的手机」. Observed on this Mac 2026-09-12: four
/// processes alive, raw.csv zero bytes, and a dialog waiting behind the window.
///
/// "Watching and not hearing your phone" and "not watching at all" both end in
/// the password, and only one of them is the feature working. That is the same
/// distinction read_presence exists to make, one layer further down.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RadioState {
    /// The scanner said poweredOn. Anything after this is about the phone.
    Scanning,
    /// Running, but too recently for silence to mean anything yet.
    Starting,
    /// Running, and the scanner has said nothing at all. Almost always a
    /// permission dialog nobody has answered.
    NoAnswer,
    Denied,
    Off,
    Unsupported,
}

/// How long the scanner may say nothing before the silence is itself the news.
/// It logs its state within a second of the radio being available; the margin
/// is for a slow launch, not for a dialog.
pub const RADIO_GRACE_S: i64 = 12;

pub fn radio_state(scan_log: Option<&str>, running_for_s: i64) -> RadioState {
    let log = scan_log.unwrap_or("");
    // Most recent word wins: a radio switched off and on again logs both.
    for line in log.lines().rev() {
        if line.contains("STATE poweredOn") {
            return RadioState::Scanning;
        }
        if line.contains("STATE unauthorized") {
            return RadioState::Denied;
        }
        if line.contains("STATE poweredOff") {
            return RadioState::Off;
        }
        if line.contains("STATE unsupported") {
            return RadioState::Unsupported;
        }
    }
    if running_for_s < RADIO_GRACE_S {
        RadioState::Starting
    } else {
        RadioState::NoAnswer
    }
}

// ---- signal calibration ---------------------------------------------------
//
// permit-bridge.sh ships -72 / -85 with a comment saying "do not ship these
// numbers"; they came from one phone on one desk. Worse, presence-pipeline.sh
// never passed REPOSE_NEAR_DBM/REPOSE_FAR_DBM at all, so no value anyone chose
// could ever have reached the bridge.
//
// The measurement is two walks: stand where you work, then walk away and stay
// away. What comes back is two clouds of dBm, and the only question worth
// asking of them is whether they are far enough apart to tell apart.

/// How many samples each leg needs before its numbers mean anything. The
/// scanner publishes roughly one a second, so this is about twenty seconds of
/// standing still -- short enough to do twice, long enough that one reflection
/// off a filing cabinet cannot decide where your desk ends.
pub const CALIBRATION_MIN_SAMPLES: usize = 20;

/// The two clouds must be at least this far apart, in dB, before the midpoint
/// between them means anything. Below it the radio is telling us that near and
/// far look the same from here, which is a real answer and not a failure to
/// measure.
pub const CALIBRATION_MIN_GAP_DB: f64 = 12.0;

/// And at least this long, in milliseconds, between the first reading and the
/// last.
///
/// The count alone is not a proxy for time. The scanner emits near-duplicate
/// rows a millisecond apart, so twenty samples arrived in under three seconds
/// on the first live run -- twenty readings of one instant, which says nothing
/// about how the signal moves while you sit there. A standard deviation
/// computed from that is a number with no evidence behind it.
///
/// Ten seconds, not fifteen: a leg is twenty seconds long (see
/// [CALIBRATION_LEG_MS]) and macOS's scan cadence leaves gaps of up to ~7 s
/// between sightings, so fifteen could not be promised inside twenty.
pub const CALIBRATION_MIN_SPAN_MS: i64 = 10_000;

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationLeg {
    pub n: usize,
    pub mean: f64,
    pub sd: f64,
    pub min: i32,
    pub max: i32,
    /// First reading to last. See [CALIBRATION_MIN_SPAN_MS].
    pub span_ms: i64,
}

impl CalibrationLeg {
    fn of(readings: &[(i32, i64)]) -> Self {
        if readings.is_empty() {
            return Self { n: 0, mean: 0.0, sd: 0.0, min: 0, max: 0, span_ms: 0 };
        }
        let samples: Vec<i32> = readings.iter().map(|&(v, _)| v).collect();
        let span_ms = readings.iter().map(|&(_, t)| t).max().unwrap_or(0)
            - readings.iter().map(|&(_, t)| t).min().unwrap_or(0);
        let n = samples.len();
        let mean = samples.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
        // Population sd: these are all the samples there were, not a sample of
        // a larger set we could have taken.
        let var = samples.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n as f64;
        Self {
            n,
            mean,
            sd: var.sqrt(),
            min: *samples.iter().min().unwrap(),
            max: *samples.iter().max().unwrap(),
            span_ms,
        }
    }
}

// `rename_all` on an enum renames the VARIANTS. The fields inside them need
// `rename_all_fields`, and without it `near_dbm` went to a front end reading
// `nearDbm` -- which read every measured band as 0 > 0 and called it 太像.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum CalibrationOutcome {
    /// Usable thresholds, with a gap between them so a phone hovering at the
    /// boundary does not flap the lock.
    ///
    /// `far_silent`: the far leg heard too little to measure, which is not a
    /// failure -- the person walked far enough that the Mac stopped hearing
    /// the phone, and that is the clearest separation there is. FAR is then
    /// one full gap below NEAR, and the bridge's own no-beacon rule covers
    /// the rest.
    Ok { near_dbm: i32, far_dbm: i32, far_silent: bool },
    /// One or both legs are too short to say anything.
    NotEnoughSamples { near: usize, far: usize, needed: usize },
    /// Enough readings, but they all arrived at once. Standing still for three
    /// seconds is not a measurement of standing still.
    TooBrief { near_ms: i64, far_ms: i64, needed_ms: i64 },
    /// Measured fine, and the answer is that this spot cannot tell the two
    /// apart. Offering thresholds anyway would be inventing a boundary the
    /// radio never found.
    TooSimilar { gap_db: f64, needed_db: f64 },
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationResult {
    pub near: CalibrationLeg,
    pub far: CalibrationLeg,
    pub outcome: CalibrationOutcome,
}

/// Turn two walks into two thresholds, or into a reason there are none.
///
/// NEAR sits one standard deviation below the near cloud's mean and FAR one
/// above the far cloud's, so ordinary jitter inside each leg does not cross a
/// line. The band between them is the hysteresis: a phone sitting exactly at
/// the edge holds whatever state it already had instead of locking and
/// unlocking the Mac every few seconds.
pub fn calibration_verdict(near: &[(i32, i64)], far: &[(i32, i64)]) -> CalibrationResult {
    let n = CalibrationLeg::of(near);
    let f = CalibrationLeg::of(far);

    // The near leg has to be heard: silence where the person sits means the
    // Mac cannot hear the phone at all, and nothing can be derived from that.
    let outcome = if n.n < CALIBRATION_MIN_SAMPLES {
        CalibrationOutcome::NotEnoughSamples {
            near: n.n,
            far: f.n,
            needed: CALIBRATION_MIN_SAMPLES,
        }
    } else if n.span_ms < CALIBRATION_MIN_SPAN_MS {
        CalibrationOutcome::TooBrief {
            near_ms: n.span_ms,
            far_ms: f.span_ms,
            needed_ms: CALIBRATION_MIN_SPAN_MS,
        }
    } else if f.n < CALIBRATION_MIN_SAMPLES || f.span_ms < CALIBRATION_MIN_SPAN_MS {
        // The far leg was silent, or nearly. That is the person having walked
        // out of earshot, which is the best separation a room can offer -- not
        // a walk that failed. FAR goes one full gap below NEAR; between the
        // two the bridge holds its state, below FAR (or with no beacon at all)
        // the phone is gone.
        let near_dbm = (n.mean - n.sd).round() as i32;
        CalibrationOutcome::Ok {
            near_dbm,
            far_dbm: near_dbm - CALIBRATION_MIN_GAP_DB as i32,
            far_silent: true,
        }
    } else {
        // Near is the stronger signal, so its mean is the larger (less
        // negative) number. A far leg that measured stronger than the near one
        // is not a separate case: it just produces a negative gap, which fails
        // the same test, and says the same thing to the person who walked.
        let gap = n.mean - f.mean;
        if gap < CALIBRATION_MIN_GAP_DB {
            CalibrationOutcome::TooSimilar { gap_db: gap, needed_db: CALIBRATION_MIN_GAP_DB }
        } else {
            let near_dbm = (n.mean - n.sd).round() as i32;
            let far_dbm = (f.mean + f.sd).round() as i32;
            // One standard deviation each way can still swallow the whole gap
            // when both clouds are noisy. Then the "band" is inverted -- FAR
            // above NEAR -- and the bridge would read present and absent at
            // once. That is the same answer as TooSimilar arriving by a
            // different route, and it gets the same reply.
            if near_dbm <= far_dbm {
                CalibrationOutcome::TooSimilar { gap_db: gap, needed_db: CALIBRATION_MIN_GAP_DB }
            } else {
                CalibrationOutcome::Ok { near_dbm, far_dbm, far_silent: false }
            }
        }
    };

    CalibrationResult { near: n, far: f, outcome }
}

/// Pull the rssi out of the verifier's output for one key.
///
/// The file carries two kinds of line: samples, and `macstate,...` rows the
/// pipeline writes for the advertiser. Reading by position without checking
/// what a row is would turn a macstate line's second field -- a key id -- into
/// a -1 dBm reading, which is a phone pressed against the antenna.
pub fn calibration_samples(csv: &str, since_ms: i64, key_id: u8) -> Vec<(i32, i64)> {
    csv.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 9 || !f.iter().any(|c| *c == "auth=VALID") {
                return None;
            }
            let at: i64 = f[0].trim().parse().ok()?;
            if at < since_ms {
                return None;
            }
            if f[4].trim().parse::<u8>().ok()? != key_id {
                return None;
            }
            Some((f[1].trim().parse::<i32>().ok()?, at))
        })
        .collect()
}


/// Where a finished calibration lives. Next to the pairing state, not next to
/// the key: it describes this room, not this phone, and it is not a secret.
pub const CALIBRATION_FILE: &str = "calibration.json";

/// One leg of the walk, as a window into what the verifier was writing at the
/// time. Storing the window rather than the samples means finish() re-reads the
/// file and cannot disagree with it.
#[derive(Clone, Copy, Debug)]
struct CalLeg {
    from_ms: i64,
    to_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct CalState {
    near: Option<CalLeg>,
    far: Option<CalLeg>,
    /// Which leg is being walked right now, if any.
    active_near: Option<bool>,
    /// Which phone is being measured. Two phones do not look alike from the
    /// same spot, so a calibration that did not know whose it was could only
    /// ever produce one shared band -- right for one of them at best.
    key_id: u8,
}

// In memory on purpose. A calibration interrupted by quitting the app is not a
// calibration to resume -- the person and the phone have both moved since. It
// starts over, and starting over is cheap.
static CALIBRATION: std::sync::Mutex<CalState> = std::sync::Mutex::new(CalState {
    near: None,
    far: None,
    active_near: None,
    key_id: 0,
});

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn verified_csv(app: &AppHandle) -> String {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("presence-run").join("verified.csv"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationProgress {
    pub near_leg: bool,
    pub samples: usize,
    pub needed: usize,
    /// Wall-clock since this leg started, and how long it has to run. The
    /// sample count fills in seconds because the scanner bursts duplicates, so
    /// the count alone would let someone finish a leg without standing still
    /// for any length of time.
    pub elapsed_ms: i64,
    pub needed_ms: i64,
    /// Most recent reading in this leg, for something on screen that moves when
    /// the phone moves. None until the first sample arrives.
    pub latest_dbm: Option<i32>,
    /// Whether the monitor is publishing at all. Without this, a leg that
    /// collects nothing because the pipeline is down looks identical to one
    /// where the phone is simply out of range.
    pub monitor_running: bool,
}

#[derive(Debug, Deserialize)]
pub struct CalibrateArgs {
    /// "near" or "far".
    pub kind: String,
    /// The key slot of the phone being measured.
    #[serde(default)]
    pub device_id: String,
}

#[tauri::command]
pub fn unlock_calibrate_start(app: AppHandle, value: CalibrateArgs) -> Result<CalibrationProgress, UnlockError> {
    let near = match value.kind.as_str() {
        "near" => true,
        "far" => false,
        other => {
            return Err(UnlockError::new(
                UnlockErrorCode::Unsupported,
                format!("不认识的校准段「{other}」"),
            ))
        }
    };
    let key_id: u8 = value.device_id.parse().map_err(|_| {
        UnlockError::new(UnlockErrorCode::Unsupported, "不知道在给哪一部手机做校准")
    })?;
    {
        let mut st = CALIBRATION.lock().map_err(|_| lock_poisoned())?;
        // A different phone is a different measurement. Keeping the other leg
        // would average two phones into one band, which is wrong for both.
        if st.key_id != key_id {
            *st = CalState::default();
            st.key_id = key_id;
        }
        // Re-walking a leg replaces it. Appending would mix the walk you just
        // decided was wrong into the one you are doing to correct it.
        let leg = CalLeg { from_ms: now_ms(), to_ms: None };
        if near { st.near = Some(leg) } else { st.far = Some(leg) }
        st.active_near = Some(near);
    }
    unlock_calibrate_sample(app)
}

#[tauri::command]
pub fn unlock_calibrate_sample(app: AppHandle) -> Result<CalibrationProgress, UnlockError> {
    let st = *CALIBRATION.lock().map_err(|_| lock_poisoned())?;
    let near_leg = st.active_near.unwrap_or(true);
    let leg = if near_leg { st.near } else { st.far };
    let samples = match leg {
        Some(l) => calibration_samples(&verified_csv(&app), l.from_ms, st.key_id),
        None => Vec::new(),
    };
    Ok(CalibrationProgress {
        near_leg,
        samples: samples.len(),
        needed: CALIBRATION_MIN_SAMPLES,
        elapsed_ms: leg.map(|l| (now_ms() - l.from_ms).max(0)).unwrap_or(0),
        needed_ms: CALIBRATION_MIN_SPAN_MS,
        latest_dbm: samples.last().map(|&(v, _)| v),
        monitor_running: presence_running(&HostMacBackend::presence_report(&app)),
    })
}

// ---- calibration driven from the phone (design doc §05 §06) ---------------
//
// The phone says when each leg starts (cmd 4 near, cmd 5 far); this side
// samples and reports its phase to the phone through the state beacon. The
// phase is a small file that the unprivileged beacon process reads once a
// second and advertises in place of the lock state while it is fresh.
//
// The far leg never starts on its own. When the near leg has enough, this side
// writes PHASE_WAIT and stops; only the phone's second command begins the far
// leg. Sampling while the person is still walking puts the walk into the far
// set, and that is the most common way to get 「两边太像」.

/// Under presence-run, where the beacon process already reads verified.csv.
pub const CALIBRATION_PHASE_FILE: &str = "calibration-phase";

/// The three command bytes (protocol §14).
pub const CAL_CMD_NEAR: u8 = 4;
pub const CAL_CMD_FAR: u8 = 5;
/// 「远处结束，定下来」. The phone's own twenty seconds at the far spot are
/// up. It goes out from wherever the person is walking back from, because
/// the far command itself may never have reached this Mac: a far spot out of
/// earshot cannot deliver 「开始量远处」, and that is the case this byte exists
/// for. In WAIT it judges with an empty far leg; in FAR it cuts the leg short.
pub const CAL_CMD_FAR_DONE: u8 = 6;

/// State-beacon values for a calibration in progress (protocol §14).
pub const PHASE_NEAR: u8 = 2;
pub const PHASE_WAIT: u8 = 3;
pub const PHASE_FAR: u8 = 4;
pub const PHASE_OK: u8 = 5;
pub const PHASE_FAIL: u8 = 6;
pub const PHASE_SILENT: u8 = 7;

/// A leg samples for exactly this long, full stop. The phone shows the same
/// twenty-second countdown, so the two sides agree on when to sit still and
/// when to get up. A leg that ended early because it had "enough" would leave
/// the person sitting through a countdown that no longer measured anything.
pub const CALIBRATION_LEG_MS: i64 = 20_000;
/// How long 「等你走开」 stays on the air before the Mac goes back to its lock state.
const PHASE_WAIT_TTL_MS: i64 = 10 * 60_000;
/// How long a verdict stays on the air. Long enough to survive the pipeline
/// restart that a good verdict triggers.
const PHASE_VERDICT_TTL_MS: i64 = 120_000;

/// `<state> <until_ms>`. until_ms 0 means "until replaced".
pub fn phase_line(state: u8, until_ms: i64) -> String {
    format!("{state} {until_ms}")
}

/// The phase to advertise now, or None when the file is absent, malformed,
/// out of range, or expired. Anything unreadable falls back to the lock state
/// rather than to a phase the phone would act on.
pub fn parse_phase(raw: &str, now_ms: i64) -> Option<u8> {
    let mut it = raw.split_whitespace();
    let state: u8 = it.next()?.parse().ok()?;
    let until: i64 = it.next()?.parse().ok()?;
    if !(PHASE_NEAR..=PHASE_SILENT).contains(&state) {
        return None;
    }
    (until == 0 || now_ms < until).then_some(state)
}

/// A leg is over when its twenty seconds are, not when it has "enough". What
/// it heard in that time is judged afterwards by [calibration_verdict].
pub fn calibration_leg_done(elapsed_ms: i64) -> bool {
    elapsed_ms >= CALIBRATION_LEG_MS
}

/// What the beacon says after a verdict. Overlap is 「两边太像」; too few or too
/// brief means this Mac could not hear the phone, which is a different fix.
pub fn phase_for_outcome(o: &CalibrationOutcome) -> u8 {
    match o {
        CalibrationOutcome::Ok { .. } => PHASE_OK,
        CalibrationOutcome::TooSimilar { .. } => PHASE_FAIL,
        CalibrationOutcome::NotEnoughSamples { .. } | CalibrationOutcome::TooBrief { .. } => PHASE_SILENT,
    }
}

/// Calibration commands (4, 5 and 6) new since `after_ms`, with the key slot
/// of the phone that sent them: (cmd, at_ms, key_id). Same rows as the
/// shortcut watcher reads, same VALID-only rule.
pub fn calibration_commands_since(csv: &str, after_ms: i64) -> Vec<(u8, i64, u8)> {
    csv.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() < 9 {
                return None;
            }
            let field = |name: &str| fields.iter().find_map(|f| f.trim().strip_prefix(name).map(str::trim));
            if field("auth=") != Some("VALID") {
                return None;
            }
            let cmd: u8 = field("cmd=")?.parse().ok()?;
            if cmd != CAL_CMD_NEAR && cmd != CAL_CMD_FAR && cmd != CAL_CMD_FAR_DONE {
                return None;
            }
            let at: i64 = fields[0].trim().parse().ok()?;
            let key_id: u8 = fields[4].trim().parse().ok()?;
            (at > after_ms).then_some((cmd, at, key_id))
        })
        .collect()
}

fn phase_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("presence-run").join(CALIBRATION_PHASE_FILE))
}

pub fn write_calibration_phase(app: &AppHandle, state: u8, until_ms: i64) {
    if let Some(p) = phase_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, phase_line(state, until_ms));
    }
}

/// Close the leg being walked without judging anything yet.
fn calibrate_close_leg() {
    if let Ok(mut st) = CALIBRATION.lock() {
        if let Some(near) = st.active_near {
            let stamp = now_ms();
            if near {
                if let Some(l) = st.near.as_mut() {
                    l.to_ms = Some(stamp)
                }
            } else if let Some(l) = st.far.as_mut() {
                l.to_ms = Some(stamp)
            }
        }
        st.active_near = None;
    }
}

/// Which leg the phone-driven calibration is in: 0 idle, else a PHASE_ value.
static CAL_DRIVE: std::sync::Mutex<u8> = std::sync::Mutex::new(0);

fn drive_set(v: u8) {
    *CAL_DRIVE.lock().unwrap_or_else(|e| e.into_inner()) = v;
}

/// Whether a command byte is in turn, given the driver's phase.
///
/// 4 (near) is always in turn -- 「再量一次」 restarts from the beginning --
/// unless a near leg is already being walked. 5 (far) only while the near
/// leg is waiting for the walk. 6 (far done) while waiting, because the far
/// command never arrived from an out-of-earshot spot, or while the far leg is
/// being sampled, to cut it short. Anything else is not a calibration byte.
pub fn calibration_in_turn(phase: u8, cmd: u8) -> bool {
    match cmd {
        CAL_CMD_NEAR => phase != PHASE_NEAR,
        CAL_CMD_FAR => phase == PHASE_WAIT,
        CAL_CMD_FAR_DONE => phase == PHASE_WAIT || phase == PHASE_FAR,
        _ => false,
    }
}

/// Set by 「远处结束」 while the far leg is being sampled; the sampling loop
/// polls it once a second and stops early. Cleared when a far leg starts.
static CAL_CUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the closed near leg heard enough to be judged at all -- the same
/// count and span the verdict will demand, asked early so the phone learns
/// 「没听到手机」 before a walk that cannot help.
fn near_leg_heard(app: &AppHandle) -> bool {
    let (leg, key_id) = match CALIBRATION.lock() {
        Ok(st) => (st.near, st.key_id),
        Err(_) => return false,
    };
    let Some(l) = leg else { return false };
    let until = l.to_ms.unwrap_or(i64::MAX);
    let samples: Vec<(i32, i64)> = calibration_samples(&verified_csv(app), l.from_ms, key_id)
        .into_iter()
        .filter(|&(_, at)| at <= until)
        .collect();
    let n = CalibrationLeg::of(&samples);
    n.n >= CALIBRATION_MIN_SAMPLES && n.span_ms >= CALIBRATION_MIN_SPAN_MS
}

/// Judge what the two legs heard and put the verdict on the air.
///
/// On success finish() restarts the pipeline to load the new thresholds; the
/// phase file outlives that restart, so the new beacon process still reports
/// the verdict.
fn drive_judge(app: &AppHandle) {
    let phase = match unlock_calibrate_finish(app.clone()) {
        Ok(r) => phase_for_outcome(&r.outcome),
        Err(_) => PHASE_SILENT,
    };
    write_calibration_phase(app, phase, now_ms() + PHASE_VERDICT_TTL_MS);
    drive_set(0);
}

/// The phone sent a calibration byte. Returns false when it is out of turn.
pub fn drive_calibration(app: AppHandle, key_id: u8, cmd: u8) -> bool {
    {
        let mut d = CAL_DRIVE.lock().unwrap_or_else(|e| e.into_inner());
        if !calibration_in_turn(*d, cmd) {
            return false;
        }
        match cmd {
            CAL_CMD_NEAR => *d = PHASE_NEAR,
            CAL_CMD_FAR => {
                CAL_CUT.store(false, std::sync::atomic::Ordering::SeqCst);
                *d = PHASE_FAR;
            }
            _ => {
                // Far done. Sampling in progress: tell that thread to stop and
                // judge; it owns the rest. Still waiting: the far command never
                // came, so judge now with an empty far leg -- that IS the
                // measurement, and it says the far spot is out of earshot.
                if *d == PHASE_FAR {
                    CAL_CUT.store(true, std::sync::atomic::Ordering::SeqCst);
                    return true;
                }
                *d = PHASE_FAR;
                drop(d);
                std::thread::spawn(move || drive_judge(&app));
                return true;
            }
        }
    }
    let near = cmd == CAL_CMD_NEAR;
    std::thread::spawn(move || {
        let kind = if near { "near" } else { "far" };
        let args = CalibrateArgs { kind: kind.into(), device_id: key_id.to_string() };
        if unlock_calibrate_start(app.clone(), args).is_err() {
            drive_set(0);
            return;
        }
        write_calibration_phase(&app, if near { PHASE_NEAR } else { PHASE_FAR }, 0);
        // The full twenty seconds, whatever arrives in them. Only 「远处结束」
        // ends the far leg early: the phone's own countdown is up and the
        // person is walking back.
        let started = now_ms();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            if calibration_leg_done(now_ms() - started) {
                break;
            }
            if !near && CAL_CUT.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
        }
        if near {
            calibrate_close_leg();
            // Not judged yet, but a near leg that heard nothing has already
            // failed, and the phone should hear that now rather than after a
            // walk that cannot help.
            if !near_leg_heard(&app) {
                write_calibration_phase(&app, PHASE_SILENT, now_ms() + PHASE_VERDICT_TTL_MS);
                drive_set(0);
                return;
            }
            write_calibration_phase(&app, PHASE_WAIT, now_ms() + PHASE_WAIT_TTL_MS);
            drive_set(PHASE_WAIT);
            return;
        }
        drive_judge(&app);
    });
    true
}

#[tauri::command]
pub fn unlock_calibrate_finish(app: AppHandle) -> Result<CalibrationResult, UnlockError> {
    let st = {
        let mut st = CALIBRATION.lock().map_err(|_| lock_poisoned())?;
        if let Some(near) = st.active_near {
            let stamp = now_ms();
            if near {
                if let Some(l) = st.near.as_mut() { l.to_ms = Some(stamp) }
            } else if let Some(l) = st.far.as_mut() {
                l.to_ms = Some(stamp)
            }
        }
        st.active_near = None;
        *st
    };

    let csv = verified_csv(&app);
    // Readings carry their own timestamps now, so the leg is a plain window
    // filter rather than a zip against a second pass over the same file --
    // which is what made it possible to lose the times in the first place.
    let take = |leg: Option<CalLeg>| -> Vec<(i32, i64)> {
        let Some(l) = leg else { return Vec::new() };
        let until = l.to_ms.unwrap_or(i64::MAX);
        calibration_samples(&csv, l.from_ms, st.key_id)
            .into_iter()
            .filter(|&(_, at)| at <= until)
            .collect()
    };

    let result = calibration_verdict(&take(st.near), &take(st.far));

    // Only a usable answer is written. A failed calibration must leave the last
    // good one in place rather than replacing it with nothing -- otherwise one
    // bad walk silently reverts the Mac to the placeholder numbers.
    if let CalibrationOutcome::Ok { near_dbm, far_dbm, far_silent } = result.outcome {
        if let Ok(dir) = app.path().app_data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(CALIBRATION_FILE);
            // Merge, never replace: calibrating a second phone must not wipe the
            // band the first one is relying on.
            let mut doc: serde_json::Value = std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            if !doc.get("byKey").is_some_and(|b| b.is_object()) {
                doc["byKey"] = serde_json::json!({});
            }
            doc["byKey"][st.key_id.to_string()] = serde_json::json!({
                "nearDbm": near_dbm,
                "farDbm": far_dbm,
                // Recorded so the panel can say 「走开之后就听不到了」 rather
                // than showing a far number nobody measured.
                "farSilent": far_silent,
                "measuredAt": HostMacBackend::now_iso(),
                "near": { "n": result.near.n, "mean": result.near.mean, "sd": result.near.sd },
                "far": { "n": result.far.n, "mean": result.far.mean, "sd": result.far.sd },
            });
            let _ = std::fs::write(&path, doc.to_string());
        }
        // The bridge reads its thresholds once, at startup. Without this the
        // numbers sit in a file and the Mac goes on using the placeholders --
        // which is exactly the state presence-pipeline.sh was already in.
        let restart = app.clone();
        std::thread::spawn(move || {
            let _ = set_presence_running(&restart, false);
            let _ = set_presence_running(&restart, true);
        });
    }
    Ok(result)
}

/// This Mac's four-hex-digit id, as the phone sees it.
///
/// Read back out of the verifier's own output rather than recomputed here.
/// presence-verify derives it from the hardware UUID and puts it in the
/// authenticated message; a second implementation in Rust would be a second
/// thing that can drift, and a Mac whose panel shows one id while its beacon
/// carries another is worse than a panel that shows none.
pub fn mac_identity(csv: &str) -> Option<String> {
    csv.lines()
        .rev()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            (f.len() == 6 && f[0] == "macstate").then(|| f[5].trim().to_uppercase())
        })
        .find(|id| id.len() == 4 && id.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Every calibrated band, as `keyId:near:far` pairs for permit-bridge.sh.
///
/// Per phone, because two phones do not look the same to one Mac from the same
/// spot: transmit power differs by model, and a phone in a pocket is several dB
/// down from one on the desk. One shared pair means calibrating for one of them
/// and being wrong about the other.
///
/// A band with near <= far is dropped rather than passed on: it would tell the
/// bridge that one reading is both near and far, and the file is plain JSON
/// anyone can mistype.
pub fn calibration_bands(raw: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else { return String::new() };
    let mut out: Vec<String> = Vec::new();

    let mut push = |id: &str, near: i64, far: i64| {
        if id.parse::<u8>().is_ok_and(|n| n != 0) && near > far {
            out.push(format!("{id}:{near}:{far}"));
        }
    };

    if let Some(by_key) = v.get("byKey").and_then(|b| b.as_object()) {
        for (id, band) in by_key {
            if let (Some(n), Some(f)) = (
                band.get("nearDbm").and_then(|x| x.as_i64()),
                band.get("farDbm").and_then(|x| x.as_i64()),
            ) {
                push(id, n, f);
            }
        }
    }
    out.sort();
    out.join(",")
}

/// The bands recorded on this Mac, or "" when nothing has been calibrated.
pub fn calibrated_bands(app: &AppHandle) -> String {
    app.path()
        .app_data_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join(CALIBRATION_FILE)).ok())
        .map(|raw| calibration_bands(&raw))
        .unwrap_or_default()
}

fn lock_poisoned() -> UnlockError {
    UnlockError::new(UnlockErrorCode::Unsupported, "量距离的记录丢了。再量一次。")
}


// ---- process helpers -----------------------------------------------------

/// Capture stdout AND stderr.
///
/// `sysadminctl` writes its answer to stderr, in the os_log format with a
/// timestamp and pid in front of it. Reading stdout got an empty string, which
/// matched none of the expected wordings, so lock_readiness fell through to
/// "your Mac has a lock delay" -- on a Mac whose delay is immediate. The error
/// was confidently wrong, which is worse than no error, and it was reported to
/// the user as something to go and fix.
fn run_capture_all(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let text = text.trim().to_string();
    text.is_empty().then_some(()).map_or(Some(text), |_| None)
}

fn run_capture(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn run_capture_stdin(bin: &str, args: &[&str], stdin_data: &str) -> Option<String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(stdin_data.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run one privileged command and say what actually happened.
///
/// `run_status` collapses every failure into false, and the install path turned
/// that into "没有拿到管理员授权，系统里什么都没有改" -- shown to someone who had
/// just typed their password correctly. Being told you did not authorize
/// something you did authorize is worse than a bare error: it sends you looking
/// in the wrong place, and it is the same class of false statement this project
/// keeps finding in its own documents.
///
/// Cancelling really is different from failing, so the two are distinguished:
/// osascript reports a cancelled authorization as AppleScript error -128.
fn run_privileged(script: &str) -> Result<(), UnlockError> {
    // Run the AppleScript IN THIS PROCESS, not by shelling out to osascript.
    //
    // macOS attributes an authorization prompt to the executable that asks. Via
    // `/usr/bin/osascript` the box is titled "osascript" -- a name the user has
    // no reason to recognise, at the one moment in this product where they are
    // asked for an administrator password. "Do not give your password to
    // software you do not recognise" is a good habit, and we were training them
    // out of it. NSAppleScript from inside the app makes the prompt say Outsie.
    //
    // Cancelling is still distinguished from failing: AppleScript reports a
    // cancelled authorization as error -128, and a script that ran and failed
    // comes back with its own message rather than a guess about it.
    use objc2::rc::Retained;
    use objc2::AllocAnyThread;
    use objc2_foundation::{NSAppleScript, NSString};

    let source = NSString::from_str(script);
    // SAFETY / THREADING: Apple documents NSAppleScript as not thread-safe.
    //
    // NOTHING reaches this from the main thread any more. `do shell script ...
    // with administrator privileges` does not return until the dialog is
    // answered, so a caller on the main thread freezes the window for as long
    // as the dialog is up -- and for good if it is dismissed without an answer
    // or ends up behind another window. That happened on 2026-09-12: the app was
    // found alive with zero windows, stuck here, unrecoverable without a kill.
    //
    // Every command that can land here is async over spawn_blocking now, which
    // is the shape the presence pipeline has used from the start for the same
    // reason -- its script runs for as long as monitoring does.
    //
    // One instance, one thread, never shared: that is the shape that is safe.
    // It has been exercised on this Mac many times; if it ever misbehaves, the
    // alternative is Authorization Services directly, which is more code and
    // the same dialog.
    let result = unsafe {
        let apple_script = NSAppleScript::initWithSource(NSAppleScript::alloc(), &source)
            .ok_or_else(|| {
                UnlockError::new(UnlockErrorCode::InstallFailed, "无法准备授权请求")
            })?;
        let mut error_dict: Option<Retained<objc2_foundation::NSDictionary<NSString>>> = None;
        // executeAndReturnError returns a descriptor on success; on failure it
        // fills the error dictionary. The dictionary being absent is what
        // "worked" means here.
        let _ = apple_script.executeAndReturnError(Some(&mut error_dict));
        (error_dict.is_none(), error_dict)
    };

    match result {
        (true, _) => Ok(()),
        (false, err) => {
            let detail = err
                .map(|d| format!("{d:?}"))
                .unwrap_or_else(|| String::new());
            if detail.contains("-128") || detail.contains("User canceled") {
                Err(UnlockError::new(
                    UnlockErrorCode::AuthorizationDenied,
                    "取消了管理员授权，系统里什么都没有改",
                ))
            } else if detail.is_empty() {
                Err(UnlockError::new(
                    UnlockErrorCode::InstallFailed,
                    "安装脚本失败了，但没有留下说明",
                ))
            } else {
                Err(UnlockError::new(UnlockErrorCode::InstallFailed, detail))
            }
        }
    }
}

fn run_status(bin: &str, args: &[&str]) -> Result<bool, String> {
    Command::new(bin)
        .args(args)
        .status()
        .map(|s| s.success())
        .map_err(|e| e.to_string())
}

/// Escape a path for embedding inside an AppleScript `do shell script` string.
fn shell_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Find the directory holding install.sh etc. — the bundle's resource dir in a
/// packaged app, or a repo-relative dev fallback.
/// What removal actually left behind, as read from disk afterwards.
///
/// Separated from the doing so the report can be tested, and because the
/// previous version did not read anything at all: `bundle_removed` and
/// `keys_removed` were the literal `true`. The removal screen told the user
/// their presence key had been deleted while it sat in /var/db/repose-unlock,
/// because nobody looked. A report is a claim about the world; this one is now
/// derived from the world.
#[derive(Clone, Debug, PartialEq)]
pub struct RemovalFacts {
    pub rule_now: String,
    pub bundle_present: bool,
    pub key_present: bool,
    pub permit_dir_present: bool,
    pub support_dir_present: bool,
}

pub fn uninstall_report(now_iso: String, f: &RemovalFacts) -> UninstallReport {
    let still_referenced = f.rule_now.contains(SUBRULE_NAME);
    let mut residual = Vec::new();
    if still_referenced {
        residual.push(format!("锁屏规则仍引用 {SUBRULE_NAME}"));
    }
    if f.bundle_present {
        residual.push(format!("组件还在 {BUNDLE_PATH}"));
    }
    if f.key_present {
        // Named first among the leftovers when it happens: the others are inert
        // files, this one is a secret the phone still authenticates with.
        residual.push(format!("配对密钥还在 {PRESENCE_KEY_DIR}"));
    }
    if f.permit_dir_present {
        residual.push("permit 目录还在 /var/run/repose-spike".into());
    }
    if f.support_dir_present {
        residual.push("支持目录还在 /Library/Application Support/ReposeSpike".into());
    }
    UninstallReport {
        read_at: now_iso,
        rule_now: f.rule_now.clone(),
        backup_used: !still_referenced,
        diff_against_backup: vec![],
        right_removed: !still_referenced,
        bundle_removed: !f.bundle_present,
        keys_removed: !f.key_present,
        residual,
    }
}

/// Where the running pipeline's pid is recorded.
///
/// A pidfile rather than a handle in memory, because the pipeline outlives any
/// single run of this app: quit Outsie with presence running and something is
/// still scanning and still writing permits. Without a record on disk the next
/// launch cannot find it, and "start" would quietly add a second scanner
/// fighting the first over the radio.
pub const PID_FILE: &str = "presence.pid";

/// What to do when asked to start or stop.
#[derive(Debug, PartialEq)]
pub enum PipelineAction {
    /// Alive but no longer publishing. Stop it, then start a new one.
    Restart(u32),
    Start,
    Stop(u32),
    /// Already in the requested state. Starting twice would put two scanners on
    /// one radio; stopping nothing is merely pointless.
    Nothing,
}

/// Decide from a pidfile and whether that process is alive.
///
/// Separated from the doing so the awkward cases are testable: a pidfile left
/// behind by a crash, a pid that has been recycled by something else, a file
/// full of nonsense.
pub fn pipeline_action(
    pidfile: Option<&str>,
    alive: bool,
    want_running: bool,
    // Is the pipeline still publishing? See the wedged case below.
    publishing: bool,
) -> PipelineAction {
    let pid = pidfile.and_then(read_pid);
    match (pid, alive, want_running) {
        // A pidfile whose process is gone is a crash, not a running pipeline.
        (Some(_), false, true) | (None, _, true) => PipelineAction::Start,
        // Alive but silent: the user is looking at a switch that says OFF --
        // because the panel reads the same silence -- and asking for ON. This
        // used to answer Nothing, which left the control dead: the UI said
        // stopped, the backend said already running, and clicking did neither.
        // Restarting is the only answer that can agree with the screen.
        (Some(p), true, true) if !publishing => PipelineAction::Restart(p),
        (Some(_), true, true) => PipelineAction::Nothing,
        (Some(p), true, false) => PipelineAction::Stop(p),
        (Some(_), false, false) | (None, _, false) => PipelineAction::Nothing,
    }
}

pub fn read_pid(text: &str) -> Option<u32> {
    let t = text.trim();
    // Refuse 0 and 1: killing pid 1 is not a mistake worth making recoverable.
    match t.parse::<u32>() {
        Ok(p) if p > 1 => Some(p),
        _ => None,
    }
}

/// Where the bridge publishes, and where the app looks. One definition, so the
/// two cannot drift into a panel that reads a file nobody writes.
pub fn status_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(STATUS_FILE))
}

/// Where the BLE tools live: presence-pipeline.sh and the two Swift binaries.
///
/// Separate from the installer's directory because they come from different
/// places in the repo, and because the dev fallback has to point somewhere
/// different. In a packaged app both land under Resources/scripts.
pub(crate) fn resolve_ble_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(res) = app.path().resource_dir() {
        let c = res.join("scripts");
        if c.join("presence-pipeline.sh").exists() {
            return Some(c);
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/ble-spike/mac");
    dev.join("presence-pipeline.sh").exists().then_some(dev)
}

fn pid_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(PID_FILE))
}

/// Is this pid still running? `kill -0` answers without signalling.
fn pid_alive(pid: u32) -> bool {
    run_status("/bin/kill", &["-0", &pid.to_string()]).unwrap_or(false)
}

/// Start or stop the presence pipeline to match `want_running`.
///
/// The pipeline needs one administrator authorization for its privileged half.
/// That prompt is the user's to answer, which is why this is driven by the
/// panel's switch and never by a background refresh.
pub fn set_presence_running(app: &AppHandle, want_running: bool) -> Result<(), UnlockError> {
    let pidfile = pid_path(app);
    let text = pidfile.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    let alive = text.as_deref().and_then(read_pid).map(pid_alive).unwrap_or(false);

    // Publishing, not merely alive. The panel reads the same status file to
    // decide what the switch shows, so this must read it too -- otherwise the
    // two disagree and the control dies between them.
    let publishing = matches!(
        HostMacBackend::presence_report(app),
        PresenceReport::Fresh { .. } | PresenceReport::NoAuthorization
    );

    match pipeline_action(text.as_deref(), alive, want_running, publishing) {
        PipelineAction::Nothing => Ok(()),
        PipelineAction::Restart(pid) => {
            let _ = run_status("/bin/kill", &["-TERM", &pid.to_string()]);
            if let Some(p) = pid_path(app) {
                let _ = std::fs::remove_file(p);
            }
            // Give the old chain a moment to run its handlers -- the bridge
            // clears the permit on the way out, and two pipelines briefly
            // sharing one radio is worth avoiding.
            std::thread::sleep(std::time::Duration::from_secs(2));
            start_pipeline(app)
        }
        PipelineAction::Stop(pid) => {
            // TERM, so the bridge's handler gets a chance to clear the permit.
            // If it does not reach it, the plugin ages the permit out within
            // PERMIT_FRESHNESS_S regardless -- see permit-bridge.sh.
            let _ = run_status("/bin/kill", &["-TERM", &pid.to_string()]);
            if let Some(p) = pidfile {
                let _ = std::fs::remove_file(p);
            }
            Ok(())
        }
        PipelineAction::Start => start_pipeline(app),
    }
}

/// Spawn a fresh pipeline. Shared by Start and Restart.
fn start_pipeline(app: &AppHandle) -> Result<(), UnlockError> {
            let dir = resolve_ble_dir(app).ok_or_else(|| {
                UnlockError::new(UnlockErrorCode::Unsupported, "找不到在场监测的程序")
            })?;
            let status = status_path(app).ok_or_else(|| {
                UnlockError::new(UnlockErrorCode::Unsupported, "找不到可写的应用数据目录")
            })?;
            let work = app
                .path()
                .app_data_dir()
                .map(|d| d.join("presence-run"))
                .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?;
            let _ = std::fs::create_dir_all(&work);

    // Name this run's flag before spawning, so the outgoing pipeline -- which
    // may still be inside its four-second teardown -- cannot delete it. See the
    // comment on RUNFLAG in presence-pipeline.sh.
    // The app's pid alone is not unique: two restarts inside one app session
    // would share a name and reintroduce exactly the race this closes.
    static RUN_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = RUN_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let runflag = work.join(format!("running.{}.{seq}", std::process::id()));
    let _ = std::fs::remove_file(&runflag);

    // Per-phone bands. The single NEAR/FAR pair is gone: with several phones
    // there is no one pair that is right for all of them, and the bridge keeps
    // its own conservative defaults for any key with no entry.
    let bands = calibrated_bands(app);

    let child = Command::new("/bin/bash")
        .arg(dir.join("presence-pipeline.sh"))
        .arg("0") // run until stopped
        .env("REPOSE_RUNFLAG", &runflag)
        .env("REPOSE_STATUS_FILE", &status)
        // Empty when nothing has been calibrated, which leaves the bridge on
        // its own conservative defaults rather than on zeroes.
        .env("REPOSE_BANDS", &bands)
        .env("REPOSE_DISABLED_FILE", work.join("disabled-keys"))
        .env("REPOSE_PIPELINE_DIR", &work)
        // We raise the authorization ourselves, below.
        .env("REPOSE_SKIP_PRIVILEGED", "1")
        // Local target: the plugin is on this machine, so the permit is a
        // local root-owned file and the whole privileged half is one
        // prompt. REPOSE_SSH being absent is what selects that.
        .env_remove("REPOSE_SSH")
        // Its own log, so a refusal has somewhere to be read from. Inherited
        // stderr goes to the app's, which is nowhere.
        .stderr(
            std::fs::File::create(work.join("pipeline.log"))
                .map(std::process::Stdio::from)
                .unwrap_or_else(|_| std::process::Stdio::null()),
        )
        .spawn()
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?;

    // Wait for the pipeline to lay out its files before root goes looking for
    // them. The run flag is the last thing it creates before it would have
    // asked for root itself.
    for _ in 0..40 {
        if runflag.exists() { break }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // No flag means the unprivileged half exited, and asking for an
    // administrator password to start a root chain that has nothing to attach
    // to is worse than useless.
    //
    // This was silent: the pipeline refused to start (its own check still
    // required key slot 1, which pair-v3 made wrong), the switch flicked back
    // to off, and nothing on screen said a word. The reason was in the script's
    // stderr, which went to the app's, which nobody reads.
    if !runflag.exists() {
        let why = std::fs::read_to_string(work.join("pipeline.log"))
            .ok()
            .and_then(|s| s.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string))
            .unwrap_or_else(|| "监测程序没有说明原因".to_string());
        return Err(UnlockError::new(
            UnlockErrorCode::Unsupported,
            format!("监测没能启动，所以没有向你要密码。{why}"),
        ));
    }

    // Raise the administrator prompt HERE, so it says Outsie.
    //
    // macOS attributes the dialog to the executable that asks. Through
    // /usr/bin/osascript the box is titled "osascript" -- a name nobody
    // installed, shown at the one moment in this product where an
    // administrator password is being typed. Asking in-process is the whole
    // difference, and it is the same call the installer and the pairing key
    // write already use.
    //
    // The command is BUILT here from paths this process owns, never read back
    // from a file the pipeline wrote: handing our authorization to a string
    // that arrived from a user-writable file is the exact shape of the
    // escalation presence-privileged.sh exists to close.
    //
    // On its own thread, because `do shell script` does not return until the
    // command does, and this one runs for as long as presence monitoring does.
    // That blocking is what holds the root chain alive -- exactly what keeping
    // osascript in the foreground used to do.
    let script = format!(
        // NEAR/FAR belong HERE, not only on the pipeline child's environment.
        // In local mode presence-pipeline.sh is told to skip the privileged
        // half (REPOSE_SKIP_PRIVILEGED=1) and this string is what actually
        // starts it -- so variables set only on the child never reach the
        // bridge. Adding them there and testing that they were "set somewhere"
        // is how the first attempt passed its own test while the bridge went on
        // using the placeholders it had been using all along.
        "do shell script \"REPOSE_MODE=local REPOSE_BIN={bin} REPOSE_KEY_DIR={keys} \
         REPOSE_RAW={raw} REPOSE_VERIFIED={verified} REPOSE_RUNFLAG={flag} \
         REPOSE_PERMIT_DIR={permit} REPOSE_STATUS_FILE={status} REPOSE_LOG_DIR={work} \
         REPOSE_BANDS={bands} REPOSE_DISABLED_FILE={disabled} \
         {priv_sh}\" with administrator privileges",
        bin = applescript_quote(&dir.to_string_lossy()),
        keys = applescript_quote(PRESENCE_KEY_DIR),
        raw = applescript_quote(&work.join("raw.csv").to_string_lossy()),
        verified = applescript_quote(&work.join("verified.csv").to_string_lossy()),
        flag = applescript_quote(&runflag.to_string_lossy()),
        permit = applescript_quote("/var/run/repose-spike"),
        status = applescript_quote(&status.to_string_lossy()),
        work = applescript_quote(&work.to_string_lossy()),
        bands = applescript_quote(&bands),
        disabled = applescript_quote(&work.join("disabled-keys").to_string_lossy()),
        priv_sh = applescript_quote(&dir.join("presence-privileged.sh").to_string_lossy()),
    );
    std::thread::spawn(move || {
        // A failure here is not silent: the pipeline checks for verify.log and
        // publishes `noauth` when the privileged half never started, which is
        // what the panel reads.
        let _ = run_privileged(&script);
    });

    if let Some(p) = pid_path(app) {
        let _ = std::fs::write(p, child.id().to_string());
    }

    // Wait for the first heartbeat before saying we started.
    //
    // This returned the moment the process was spawned, so the snapshot taken
    // straight afterwards read a status file the bridge had not written yet.
    // The switch stayed grey, and pressing it a second time turned it on --
    // by which point the first press had, in fact, worked. The user pressed
    // twice and concluded the first press was ignored, which is a fair reading
    // of what they were shown.
    //
    // The privileged half is behind an authorization dialog, so the wait has to
    // cover a human typing a password. Twenty-five seconds, polled; a start
    // that has not published by then really has not started, and the panel says
    // so rather than showing a switch whose state is a guess.
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if matches!(
            HostMacBackend::presence_report(app),
            PresenceReport::Fresh { .. } | PresenceReport::NoAuthorization
        ) {
            return Ok(());
        }
    }
    Err(UnlockError::new(
        UnlockErrorCode::Unsupported,
        "在场监测启动了，但一直没有报告状态。可能是管理员密码框被取消了，再试一次。",
    ))
}

/// Single-quote a path for a shell command that is itself inside an AppleScript
/// string literal.
///
/// Two layers of quoting, which is how the old version of this ended up
/// generating a script at run time instead. Paths here come from the app's own
/// directories, but a home folder with an apostrophe in it is ordinary and
/// would otherwise end the quoting early.
fn applescript_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn resolve_scripts_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(res) = app.path().resource_dir() {
        let candidate = res.join("scripts");
        if candidate.join("install.sh").exists() {
            return Some(candidate);
        }
        if res.join("install.sh").exists() {
            return Some(res);
        }
    }
    // Dev fallback: <crate>/../native/macos/minimal-auth-plugin
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../native/macos/minimal-auth-plugin");
    if dev.join("install.sh").exists() {
        return Some(dev);
    }
    None
}

// ---- open a settings pane ------------------------------------------------

fn open_url(url: &str) {
    let _ = Command::new("/usr/bin/open").arg(url).status();
}

// ---- Tauri commands (§6.2) -----------------------------------------------

#[tauri::command]
pub fn unlock_get_snapshot(app: AppHandle) -> Result<UnlockSnapshot, UnlockError> {
    HostMacBackend::new(&app).get_snapshot()
}

#[tauri::command]
pub fn unlock_preflight(app: AppHandle) -> Result<PreflightReport, UnlockError> {
    HostMacBackend::new(&app).preflight()
}

// EVERY COMMAND THAT CAN RAISE AN ADMINISTRATOR PROMPT IS ASYNC.
//
// `NSAppleScript ... with administrator privileges` does not return until the
// dialog is answered, and a synchronous Tauri command runs on the MAIN THREAD.
// So the window froze for as long as the dialog was up -- and if the dialog was
// dismissed without being answered, or ended up behind something, it froze for
// good: 2026-09-12 the app was found with zero windows, alive, stuck in
// unlock_pair_confirm -> run_privileged -> executeAndReturnError, unrecoverable
// without killing it.
//
// unlock_presence_set was fixed for this in 4d2330f; these four were the same
// shape and were missed, which is the useful half of the lesson -- the fix
// belonged to a CLASS of command, not to the one that was noticed.
//
// spawn_blocking, not plain async: everything inside is blocking I/O, and an
// async worker would be blocked instead of the main thread.
#[tauri::command]
pub async fn unlock_install(app: AppHandle, value: InstallArgs) -> Result<UnlockSnapshot, UnlockError> {
    let variant = match value.variant.as_deref() {
        Some("A") => Some(RuleVariant::A),
        Some("B") => Some(RuleVariant::B),
        _ => None,
    };
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || HostMacBackend::new(&handle).install(variant))
        .await
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?
}

#[tauri::command]
pub async fn unlock_repair(app: AppHandle, value: RepairArgs) -> Result<UnlockSnapshot, UnlockError> {
    let handle = app.clone();
    let target = value.target;
    tauri::async_runtime::spawn_blocking(move || HostMacBackend::new(&handle).repair(&target))
        .await
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?
}

#[tauri::command]
pub async fn unlock_uninstall(app: AppHandle) -> Result<UninstallReport, UnlockError> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || HostMacBackend::new(&handle).uninstall())
        .await
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?
}

/// Start or stop presence monitoring. Wired to the panel's switch, because it
/// raises an authorization prompt and a prompt must follow a deliberate action.
///
/// ASYNC, AND THAT IS NOT AN OPTIMISATION
///
/// Starting waits for the pipeline's first heartbeat before reporting success --
/// otherwise the panel reads a status file nobody has written yet, the switch
/// stays grey, and the user presses it a second time. That wait is up to
/// twenty-five seconds, because it has to cover somebody typing an
/// administrator password.
///
/// A synchronous Tauri command runs on the main thread, so that wait froze the
/// whole window: the app appeared hung for the entire time it was doing exactly
/// what it was asked. Declared async, Tauri runs it on its own runtime, and the
/// panel's existing in-flight state shows the wait instead of the app dying.
#[tauri::command]
pub async fn unlock_presence_set(
    app: AppHandle,
    value: EnabledArgs,
) -> Result<UnlockSnapshot, UnlockError> {
    let handle = app.clone();
    let enabled = value.enabled;
    // spawn_blocking, not plain async: everything inside is blocking I/O --
    // spawning a process, sleeping, stat-ing a file -- and running that on an
    // async worker would block the runtime instead of the main thread.
    tauri::async_runtime::spawn_blocking(move || {
        set_presence_running(&handle, enabled)?;
        HostMacBackend::new(&handle).get_snapshot()
    })
    .await
    .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?
}

#[tauri::command]
pub fn unlock_set_enabled(app: AppHandle, value: EnabledArgs) -> Result<UnlockSnapshot, UnlockError> {
    HostMacBackend::new(&app).set_enabled(value.enabled)
}

#[tauri::command]
pub fn unlock_pause_for(app: AppHandle, value: PauseArgs) -> Result<UnlockSnapshot, UnlockError> {
    let _ = value.minutes;
    HostMacBackend::new(&app).set_enabled(false)
}

#[tauri::command]
pub async fn unlock_revoke_device(
    app: AppHandle,
    value: DeviceArgs,
) -> Result<UnlockSnapshot, UnlockError> {
    let handle = app.clone();
    let id = value.device_id;
    tauri::async_runtime::spawn_blocking(move || HostMacBackend::new(&handle).revoke_device(&id))
        .await
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?
}

// ---- Pairing: the live half -----------------------------------------------
//
// `pair-with-phone` is a long-lived child that spans three commands: begin
// starts it, poll watches for the digits, confirm answers it. So the process
// handle has to outlive a command, which is what this holds.
//
// It deliberately does NOT hold root. The tool talks to a stranger over a
// radio; writing the key needs an administrator. Keeping those in separate
// processes is the same split `pair.sh` makes, and the reason the key travels
// as 64 hex characters on a pipe rather than being written by the thing parsing
// Bluetooth packets.

struct LivePairing {
    child: std::process::Child,
    digits_path: PathBuf,
    peer_path: PathBuf,
}

static PAIRING: std::sync::Mutex<Option<LivePairing>> = std::sync::Mutex::new(None);

/// Run directory for one pairing attempt, under the app's own data dir.
///
/// Not /tmp: the digits file is short-lived but the directory is also where a
/// future attempt's leftovers would be, and app data is somewhere we can
/// clear without guessing about other software's files.
fn pairing_dir(app: &AppHandle) -> Result<PathBuf, UnlockError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?
        .join("pairing");
    std::fs::create_dir_all(&dir)
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?;
    Ok(dir)
}

/// Short fingerprint of a key, the same way `pair.sh` and the phone compute it.
///
/// The key goes in on stdin, never on a command line: an argument list is
/// readable by every process on the machine, and this is the one moment the
/// presence key exists outside a 0600 file.
fn key_fingerprint(hex_key: &str) -> Option<String> {
    run_capture_stdin(
        "/bin/bash",
        &[
            "-c",
            "{ printf 'repose-presence-v1 fingerprint'; xxd -r -p; } \
             | shasum -a 256 | cut -c1-8 | tr 'a-f' 'A-F'",
        ],
        hex_key,
    )
    .map(|s| s.trim().to_string())
    .filter(|s| s.len() == 8)
}

#[tauri::command]
pub fn unlock_pair_begin(app: AppHandle) -> Result<PairingSession, UnlockError> {
    let mut slot = PAIRING.lock().map_err(|_| {
        UnlockError::new(UnlockErrorCode::Unsupported, format!("配对状态异常，请重启 {BRAND}"))
    })?;
    // Starting over means the previous attempt is dead to us. Leaving it
    // running would put two tools on the radio and let a stale answer land.
    if let Some(mut old) = slot.take() {
        let _ = old.child.kill();
        let _ = old.child.wait();
    }

    let dir = resolve_ble_dir(&app).ok_or_else(|| {
        UnlockError::new(UnlockErrorCode::Unsupported, "找不到配对程序")
    })?;
    let tool = dir.join("pair-with-phone");
    if !tool.exists() {
        return Err(UnlockError::new(
            UnlockErrorCode::Unsupported,
            "这个版本里没有带上配对程序",
        ));
    }

    let run = pairing_dir(&app)?;
    let digits_path = run.join("sas-digits");
    let peer_path = run.join("peer-name");
    let _ = std::fs::remove_file(&digits_path);
    let _ = std::fs::remove_file(&peer_path);
    let log = std::fs::File::create(run.join("pair.log")).ok();

    let child = Command::new(&tool)
        .arg("--digits-file")
        .arg(&digits_path)
        .arg("--peer-file")
        .arg(&peer_path)
        // Three minutes, matching the phone's own self-closing window. A longer
        // one would leave a connectable surface up after the person walked away.
        .arg("--timeout")
        .arg("180")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(log.map(std::process::Stdio::from).unwrap_or_else(std::process::Stdio::null))
        .spawn()
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?;

    *slot = Some(LivePairing { child, digits_path, peer_path });
    Ok(PairingSession::stage(PairingStage::Scanning))
}

/// Has a beacon signed with the new key arrived yet?
///
/// Reads the verified stream the presence pipeline writes. `auth=VALID` means
/// presence-verify recomputed the tag with the key on THIS Mac and it matched,
/// so the phone is holding the same one. Nothing here takes the phone's word
/// for anything; there is no word to take.
fn phone_has_used_the_key(app: &AppHandle) -> bool {
    let Ok(dir) = app.path().app_data_dir() else { return false };
    let Ok(text) = std::fs::read_to_string(dir.join("presence-run/verified.csv")) else {
        return false;
    };
    // Only the tail: the file spans the whole run, and a VALID row from before
    // this pairing would answer a question nobody asked.
    text.lines().rev().take(400).any(|l| l.contains("auth=VALID"))
}

#[tauri::command]
pub fn unlock_pair_poll(app: AppHandle) -> Result<PairingSession, UnlockError> {
    let mut slot = PAIRING.lock().map_err(|_| {
        UnlockError::new(UnlockErrorCode::Unsupported, format!("配对状态异常，请重启 {BRAND}"))
    })?;
    let Some(live) = slot.as_mut() else {
        // No exchange running. If this Mac has just written a key, the question
        // has become "has the phone caught up", which the beacons answer.
        return Ok(PairingSession::stage(PairingStage::Idle));
    };
    let digits = std::fs::read_to_string(&live.digits_path).ok();
    let exit = live
        .child
        .try_wait()
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?
        .map(|s| s.code());
    let status = pairing_stage(digits.as_deref(), exit);
    if status.stage == PairingStage::Failed {
        *slot = None;
    }
    Ok(status)
}

/// Has the phone started using the key this Mac just wrote?
///
/// Separate from unlock_pair_poll because by this point the exchange is over
/// and its child is gone -- the question has moved from "what is the tool
/// doing" to "do both ends hold the same key", and only one of those has an
/// answer on the radio.
#[tauri::command]
pub fn unlock_pair_await_phone(app: AppHandle) -> Result<PairingSession, UnlockError> {
    if phone_has_used_the_key(&app) {
        Ok(PairingSession {
            stage: PairingStage::Done,
            digits: None,
            fingerprint: None,
            peer_name: None,
            detail: Some("两边都确认了，这台 Mac 认得你的手机。".into()),
        })
    } else {
        Ok(PairingSession::stage(PairingStage::WaitingForPhone))
    }
}

/// The human said the digits match. This is the only path that writes a key.
#[tauri::command]
pub async fn unlock_pair_confirm(app: AppHandle) -> Result<PairingSession, UnlockError> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || pair_confirm_blocking(handle))
        .await
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?
}

fn pair_confirm_blocking(app: AppHandle) -> Result<PairingSession, UnlockError> {
    use std::io::{Read, Write};

    let mut live = {
        let mut slot = PAIRING.lock().map_err(|_| {
            UnlockError::new(UnlockErrorCode::Unsupported, format!("配对状态异常，请重启 {BRAND}"))
        })?;
        slot.take().ok_or_else(|| {
            UnlockError::new(UnlockErrorCode::Unsupported, "这次配对已经结束了，请重新开始")
        })?
    };

    // Answering means the digits must actually have been shown. Without this a
    // UI bug that skipped the comparison would still derive a key, which is the
    // one outcome the whole protocol exists to prevent.
    if !live.digits_path.exists() {
        let _ = live.child.kill();
        let _ = live.child.wait();
        return Err(UnlockError::new(
            UnlockErrorCode::Unsupported,
            "还没有出现要核对的数字，不能确认",
        ));
    }

    if let Some(mut stdin) = live.child.stdin.take() {
        let _ = stdin.write_all(b"y\n");
        let _ = stdin.flush();
    }

    let mut key = String::new();
    if let Some(mut out) = live.child.stdout.take() {
        let _ = out.read_to_string(&mut key);
    }
    let code = live.child.wait().ok().and_then(|s| s.code());

    // `<keyId> <phoneIdHex> <keyHex>`. The slot decides where the key goes; the
    // identity decides which older key it replaces. Both were covered by the
    // digits the human just compared, so trusting them here is trusting that
    // comparison and nothing more.
    let Some((key_id, phone_id, key, console_key)) = parse_pair_output(&key) else {
        return Ok(PairingSession::failed(pairing_failure(code)));
    };
    if code != Some(0) {
        return Ok(PairingSession::failed(pairing_failure(code)));
    }

    let fingerprint = key_fingerprint(&key);
    let peer_name = std::fs::read_to_string(&live.peer_path)
        .ok()
        .map(|s| s.trim().chars().take(60).collect::<String>())
        .filter(|s| !s.is_empty());

    // Install it. One administrator prompt, attributed to Outsie.
    //
    // The key goes via a 0600 file the unprivileged side writes, NOT
    // interpolated into the shell command. Two reasons, one of which cost a
    // working pairing:
    //
    //   - An argument list is readable by every process on the machine, and
    //     this is the one moment the presence key exists outside a root-only
    //     file.
    //   - The first version built `printf '%%s\n' <key>` with format!, on the
    //     habit that %% escapes a percent. It does not -- format! only treats
    //     {} specially -- so the shell received `printf '%%s\n'`, which prints
    //     the literal text "%s" and ignores its argument. The key file came out
    //     three bytes long, the verifier refused it, and the app reported
    //     配对完成 over a Mac that could never recognise the phone.
    let staged = pairing_dir(&app)?.join("key.hex");
    std::fs::write(&staged, &key)
        .map_err(|e| UnlockError::new(UnlockErrorCode::InstallFailed, e.to_string()))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600));
    }
    let staged_path = staged.display().to_string();
    // Slots this same phone already owns. They are replaced, not added to:
    // without this, every re-pair would leave behind a key in a slot the phone
    // no longer advertises for, and the list would fill with rows for phones
    // that are really all one phone.
    let stale: Vec<u8> = HostMacBackend::key_slots()
        .iter()
        .map(|s| s.id)
        .filter(|id| *id != key_id && HostMacBackend::slot_phone_id(*id).as_deref() == Some(phone_id.as_str()))
        .collect();
    let removals: String = stale
        .iter()
        .map(|id| format!(" && rm -f {KEY_DIR}/presence-key.{id} {KEY_DIR}/presence-key.{id}.* ", KEY_DIR = PRESENCE_KEY_DIR))
        .collect();

    let script = format!(
        "do shell script \"mkdir -p {KEY_DIR} && chown root:wheel {KEY_DIR} && chmod 755 {KEY_DIR} \
         && install -m 600 -o root -g wheel '{staged_path}' {KEY_DIR}/presence-key.{key_id} \
         && echo repose-pair-v3 > {KEY_DIR}/presence-key.{key_id}.provenance \
         && echo {phone_id} > {KEY_DIR}/presence-key.{key_id}.phone \
         && chmod 644 {KEY_DIR}/presence-key.{key_id}.provenance {KEY_DIR}/presence-key.{key_id}.phone\
         {removals}\" with administrator privileges",
        KEY_DIR = PRESENCE_KEY_DIR,
    );
    let installed = run_privileged(&script);
    // The staged copy is the key in the clear. It goes whether or not the
    // install worked.
    let _ = std::fs::remove_file(&staged);
    installed?;

    // Read back before claiming anything.
    //
    // This is the check that would have caught the printf bug at the moment it
    // happened rather than one lock screen later. The file is root-only so its
    // contents are unreadable from here, but its size is not -- and a key that
    // is not 65 bytes is not a key the verifier will take.
    //
    // The directory is 755 on purpose, which is what makes this possible.
    let key_path = format!("{PRESENCE_KEY_DIR}/presence-key.{key_id}");
    let landed = std::fs::metadata(&key_path).map(|m| m.len()).unwrap_or(0);
    if landed != (key.len() + 1) as u64 && landed != key.len() as u64 {
        return Ok(PairingSession::failed(
            "密钥没有正确写入这台 Mac，配对没有生效。请重新配对一次。",
        ));
    }

    // Pairing is not finished until this Mac is actually watching for the
    // phone. Stopping at "key written" hands back a paired Mac that does
    // nothing, and the panel would have said 已配对 above a dead pipeline.
    //
    // RESTART, not "make sure it is running".
    //
    // This was `set_presence_running(true)`, which does nothing when a pipeline
    // is already alive -- and pairing from the panel is the ordinary case where
    // one is. The verifier in that pipeline had already looked for the key
    // before it existed, so it went on refusing every beacon for the life of
    // the process. The panel said 已配对 and 监测运行中 and the Mac never
    // unlocked, with nothing anywhere saying why.
    //
    // The verifier no longer caches a missing key either; both halves are fixed
    // because either one alone leaves the other as a trap for the next change.
    // Off-thread: this waits for the pipeline's first heartbeat, and the sheet
    // has somewhere useful to be in the meantime -- it moves to
    // 还差手机上那一下 and starts listening for the phone. Blocking here froze
    // the window at the exact moment the user was being asked to go and tap
    // something on their phone.
    let restart = app.clone();
    std::thread::spawn(move || {
        let _ = set_presence_running(&restart, false);
        let _ = set_presence_running(&restart, true);
    });

    // The nickname is cosmetic, so saving it must never be able to fail the
    // pairing: a Mac that paired correctly but could not write a name is
    // paired.
    // The catalogue key, app-side and 0600.
    //
    // NOT next to the presence key: that one is root-only on purpose, and this
    // one has to be readable by the app that signs catalogues. Whoever gets
    // this file can forge a button list for this Mac; they cannot open it.
    if let Ok(dir) = pairing_dir(&app) {
        let path = dir.join(format!("console-key.{key_id}"));
        if std::fs::write(&path, &console_key).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }

    if let (Some(n), Ok(dir)) = (&peer_name, pairing_dir(&app)) {
        // Against the slot it belongs to. The shared file was correct only
        // while there was one slot; a second phone would have taken the first
        // one's name.
        let _ = std::fs::write(dir.join(format!("peer-name.{key_id}.saved")), n);
        if key_id == 1 {
            let _ = std::fs::write(dir.join("peer-name.saved"), n);
        }
    }

    Ok(PairingSession {
        stage: PairingStage::WaitingForPhone,
        digits: None,
        fingerprint,
        peer_name: peer_name.clone(),
        detail: Some(match &peer_name {
            Some(n) => format!("这台 Mac 已经记住「{n}」。还要在手机上也点一下「一样」。"),
            None => "这台 Mac 已经记下了。还要在手机上也点一下「一样」。".into(),
        }),
    })
}

#[tauri::command]
pub fn unlock_pair_cancel() {
    let Ok(mut slot) = PAIRING.lock() else { return };
    if let Some(mut live) = slot.take() {
        // Kill rather than answer "no": the tool treats any non-y as an abort
        // and exits anyway, and a dead pipe must not leave this hanging.
        let _ = live.child.kill();
        let _ = live.child.wait();
    }
}

/// Can this Mac be locked, and by what?
///
/// `sysadminctl -screenLock status` needs neither root nor a password, and its
/// answer decides whether locking is even meaningful: with a non-zero delay,
/// putting the display to sleep leaves the session UNLOCKED for the grace
/// period. The drill would then blank the screen, the user would press a key,
/// and nothing would have been tested.
///
/// Pure so the parsing is checked without a Mac in each condition -- including
/// the one that matters most, where the output is something we did not expect
/// and the safe reading is "do not claim to have locked anything".
pub fn lock_readiness(screen_lock_status: Option<&str>) -> Result<(), &'static str> {
    let Some(text) = screen_lock_status else {
        return Err("问不出这台 Mac 的锁屏设置，没有锁屏。");
    };
    if text.contains("immediate") {
        return Ok(());
    }
    // Both messages name the exact place. An error that states a problem and
    // leaves the reader to find the setting is half an error -- and this panel
    // has already shipped one link that pointed at the page the user was
    // standing on.
    if text.contains("is off") || text.contains("screenLock is off") {
        Err("这台 Mac 锁屏后不要求密码，所以「回车解锁」没有意义。\
             打开「系统设置 → 锁定屏幕 → 在屏幕保护程序开始或关闭显示器后要求输入密码」，\
             选「立即」。")
    } else {
        Err("这台 Mac 锁屏后不是立刻要密码，中间有一段时间谁都能直接用。\
             打开「系统设置 → 锁定屏幕 → 在屏幕保护程序开始或关闭显示器后要求输入密码」，\
             把它改成「立即」。")
    }
}

/// Lock the screen for the verification drill.
///
/// WHAT THIS USED TO DO, AND WHY IT DID NOTHING
///
/// It asked System Events to press control-command-Q. That needs Accessibility
/// permission for this app, which nobody had granted, so the call failed --
/// and the result was discarded with `let _`, so the button reported nothing at
/// all. Pressing 「锁屏，试一次」 did exactly nothing, visibly and repeatedly.
///
/// `pmset displaysleepnow` needs no permission. It only LOCKS when the
/// screen-lock delay is immediate, which is why lock_readiness runs first
/// rather than after -- and why a Mac that cannot be locked is told so instead
/// of being blanked.
///
/// It also has to run as the console user: this process is in that session
/// already, but the same call from the presence pipeline's root half is not,
/// which is why permit-bridge.sh wraps it in `launchctl asuser`.
///
/// NOT ScreenSaverEngine. tools/vm-spike/vm-env.sh recommends `open -a
/// ScreenSaverEngine`, verified on 14.6.1 in a VM. This Mac is macOS 26, where
/// that command returns success and does not lock -- which is how the phone's
/// lock button appeared to work in the logs and never moved the screen.
#[tauri::command]
pub fn unlock_drill_start(value: DrillArgs) -> Result<(), UnlockError> {
    let _ = value.kind;
    // stderr, not stdout -- see run_capture_all.
    let status = run_capture_all("/usr/sbin/sysadminctl", &["-screenLock", "status"]);
    lock_readiness(status.as_deref())
        .map_err(|m| UnlockError::new(UnlockErrorCode::Unsupported, m))?;

    match run_status("/usr/bin/pmset", &["displaysleepnow"]) {
        Ok(true) => Ok(()),
        _ => Err(UnlockError::new(
            UnlockErrorCode::Unsupported,
            "没有锁上屏幕。这一步不需要任何权限，失败通常意味着系统拒绝了请求。",
        )),
    }
}

/// Open the pane the lock-readiness error names.
///
/// Naming the path is better than not naming it; opening it is better still.
/// The panel already shipped one 「前往设置」 that led to the page the reader
/// was standing on, so a link here has to actually land somewhere.
#[tauri::command]
pub fn unlock_open_lock_screen_settings() {
    open_url("x-apple.systempreferences:com.apple.Lock-Screen-Settings.extension");
}

#[tauri::command]
pub fn unlock_open_bluetooth_settings() {
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Bluetooth");
}

#[tauri::command]
pub fn unlock_copy_diagnostics() -> Result<String, UnlockError> {
    let rule = HostMacBackend::read_rule().unwrap_or_else(|| "(unreadable)".into());
    let build = run_capture("/usr/bin/sw_vers", &["-buildVersion"]).unwrap_or_default();
    Ok(format!("system.login.screensaver rule = {rule}\nmacOS build = {build}"))
}

#[tauri::command]
pub fn unlock_export_manifest() -> Result<String, UnlockError> {
    Ok("Repose 手机钥匙 — 离线卸载：\n  sudo /var/db/repose-unlock/uninstall.sh\n".to_string())
}

// ---- tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_serializes_kebab_case() {
        let j = serde_json::to_string(&UnlockErrorCode::PreflightRuleShape).unwrap();
        assert_eq!(j, "\"preflight-rule-shape\"");
        let j2 = serde_json::to_string(&UnlockErrorCode::BackupMissing).unwrap();
        assert_eq!(j2, "\"backup-missing\"");
    }

    #[test]
    fn remediation_is_internally_tagged_on_kind() {
        let j = serde_json::to_string(&Remediation::ReinstallComponent).unwrap();
        assert_eq!(j, "{\"kind\":\"reinstall-component\"}");
        let j2 = serde_json::to_string(&Remediation::FixOnPhone { hint: PhoneHint::Unseen }).unwrap();
        assert_eq!(j2, "{\"kind\":\"fix-on-phone\",\"hint\":\"unseen\"}");
    }

    #[test]
    fn component_invocation_matches_ts_union() {
        let observed = serde_json::to_string(&ComponentInvocation::Observed { at: "t".into() }).unwrap();
        assert_eq!(observed, "{\"kind\":\"observed\",\"at\":\"t\"}");
        let never = serde_json::to_string(&ComponentInvocation::NeverObserved).unwrap();
        assert_eq!(never, "{\"kind\":\"never-observed\"}");
    }

    #[test]
    fn error_omits_absent_optionals() {
        let e = UnlockError::new(UnlockErrorCode::DaemonUnavailable, "x");
        let j = serde_json::to_string(&e).unwrap();
        assert_eq!(j, "{\"code\":\"daemon-unavailable\",\"detail\":\"x\"}");
    }

    #[test]
    fn snapshot_uses_camel_case_keys() {
        let s = UnlockSnapshot {
            read_at: "t".into(),
            state: UnlockState::NotInstalled,
            presence: Presence::TransportUnavailable,
            presence_running: false,
            variant: None,
            components: vec![],
            component_invocation: ComponentInvocation::NeverObserved,
            devices: Vec::new(),
            stats: UnlockStats { unlocks_today: 0, last_unlock_at: None },
            last_failure: None,
            macos_build: "b".into(),
            component_version: "0".into(),
            mac_id: None,
            radio: RadioState::Scanning,
        };
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert!(v.get("readAt").is_some());
        assert!(v.get("componentInvocation").is_some());
        assert!(v.get("macosBuild").is_some());
        assert_eq!(v.get("state").unwrap(), "not-installed");
    }

    #[test]
    fn decide_variant_reads_the_rule_array() {
        assert_eq!(decide_variant(r#"["ai.repose.spike","use-login-window-ui"]"#), Some(RuleVariant::A));
        assert_eq!(decide_variant(r#"["ai.repose.unlock","authenticate-session-owner-or-admin"]"#), Some(RuleVariant::B));
        assert_eq!(decide_variant(r#"["something-else"]"#), None);
        assert_eq!(decide_variant("not json"), None);
    }

    // ---- assess ----------------------------------------------------------
    //
    // These drive the states the panel has to get right, without putting a real
    // Mac into any of them. The fail-open one in particular must never be
    // produced on a machine anyone uses: reaching it means that machine unlocks
    // for anybody until it is repaired.

    fn facts() -> HostFacts {
        HostFacts {
            rule_references_us: true,
            bundle_present: true,
            bundle_signature_ok: Some(true),
            daemon_loaded: Some(true),
            presence_key: PresenceKeyState::Ok { paired: true },
            presence: PresenceReport::Fresh { state: "near".into(), rssi: Some(-55) },
        }
    }

    fn find(a: &Assessment, id: ComponentId) -> &UnlockComponent {
        a.components
            .iter()
            .find(|c| format!("{:?}", c.id) == format!("{id:?}"))
            .unwrap_or_else(|| panic!("no {id:?} component in {:?}", a.components))
    }

    #[test]
    fn a_clean_install_reports_everything_ok() {
        let a = assess(&facts());
        assert_eq!(a.state, UnlockState::AwaitingVerification);
        assert_eq!(find(&a, ComponentId::Rule).health, Health::Ok);
        assert_eq!(find(&a, ComponentId::Component).health, Health::Ok);
        assert_eq!(find(&a, ComponentId::Daemon).health, Health::Ok);
    }

    #[test]
    fn nothing_installed_lists_no_rule_or_component() {
        let a = assess(&HostFacts {
            rule_references_us: false,
            bundle_present: false,
            bundle_signature_ok: None,
            daemon_loaded: Some(false),
            presence_key: PresenceKeyState::Missing,
            presence: PresenceReport::NeverRan,
        });
        assert_eq!(a.state, UnlockState::NotInstalled);
        assert!(a.components.iter().all(|c| !matches!(c.id, ComponentId::Rule | ComponentId::Component)));
        // The daemon guards a rule that does not exist yet; listing it as broken
        // would be noise on a machine where nothing is wrong.
        assert!(a.components.iter().all(|c| !matches!(c.id, ComponentId::Daemon)));
    }

    #[test]
    fn a_rule_pointing_at_a_missing_bundle_is_broken_not_merely_missing() {
        // The user dragged the app to the Trash. macOS treats the step it can no
        // longer instantiate as passed, so this Mac now opens with no password.
        let a = assess(&HostFacts { bundle_present: false, bundle_signature_ok: None, ..facts() });
        assert_eq!(a.state, UnlockState::NeedsRepair);
        assert_eq!(find(&a, ComponentId::Component).health, Health::Broken);
        // The rule is implicated too: it is the half that is pointing at nothing.
        assert_eq!(find(&a, ComponentId::Rule).health, Health::Broken);
        // And it must offer a way out, not just a red dot.
        assert!(find(&a, ComponentId::Component).remediation.is_some());
        // The wording has to say what is true now, not name a missing part.
        let d = &find(&a, ComponentId::Component).detail;
        assert!(d.contains("不用密码"), "detail did not state the consequence: {d}");
    }

    #[test]
    fn an_unloadable_bundle_is_as_dangerous_as_a_missing_one() {
        // Present on disk but macOS will not load it -- same fail-open, and much
        // easier to mistake for healthy, since the file is right there.
        let a = assess(&HostFacts { bundle_signature_ok: Some(false), ..facts() });
        assert_eq!(a.state, UnlockState::NeedsRepair);
        assert_eq!(find(&a, ComponentId::Component).health, Health::Broken);
    }

    #[test]
    fn a_signature_we_could_not_check_is_unknown_not_ok_and_not_broken() {
        // "codesign would not run" must render as neither fine nor catastrophic.
        // Reporting it as broken would raise the wide-open alarm on a Mac that
        // is probably healthy, and an alarm that cries wolf stops being read --
        // which costs exactly the case it exists for.
        let a = assess(&HostFacts { bundle_signature_ok: None, ..facts() });
        assert_eq!(find(&a, ComponentId::Component).health, Health::Unknown);
        assert_ne!(a.state, UnlockState::NeedsRepair);
    }

    #[test]
    fn a_bundle_the_rule_does_not_reference_is_half_installed() {
        let a = assess(&HostFacts { rule_references_us: false, ..facts() });
        assert_eq!(a.state, UnlockState::HalfInstalled);
        assert_eq!(find(&a, ComponentId::Component).health, Health::Degraded);
        // This direction is safe -- no rule, no fail-open -- so it must not be
        // dressed up in the same red as the dangling case.
        assert!(find(&a, ComponentId::Component).detail.contains("密码照常能登录"));
    }

    #[test]
    fn a_stopped_daemon_is_degraded_and_says_what_is_unguarded() {
        let a = assess(&HostFacts { daemon_loaded: Some(false), ..facts() });
        assert_eq!(find(&a, ComponentId::Daemon).health, Health::Degraded);
        // Still installed and working -- the guard is off, the feature is not broken.
        assert_eq!(a.state, UnlockState::AwaitingVerification);
    }

    #[test]
    fn no_presence_key_is_degraded_because_it_fails_closed() {
        let a = assess(&HostFacts { presence_key: PresenceKeyState::Missing, ..facts() });
        let t = find(&a, ComponentId::Transport);
        assert_eq!(t.health, Health::Degraded);
        assert!(t.detail.contains("密码照常能登录"));
    }

    #[test]
    fn a_world_writable_presence_key_is_broken_because_it_fails_open() {
        let a = assess(&HostFacts {
            presence_key: PresenceKeyState::BadPermissions { detail: "权限=666".into() },
            ..facts()
        });
        assert_eq!(find(&a, ComponentId::Transport).health, Health::Broken);
    }

    #[test]
    fn a_dev_key_is_never_described_as_pairing() {
        // A USB-pushed key defends against nobody in the middle. Three
        // artifacts on this project have described protections the code did not
        // have; this asserts the panel is not the fourth.
        let mut f = facts();
        f.presence_key = PresenceKeyState::Ok { paired: false };
        let a = assess(&f);
        let c = find(&a, ComponentId::Transport);
        assert!(c.detail.contains("中间人"), "must disclaim the defence: {}", c.detail);
        assert!(c.remediation.is_some(), "must offer a way to get a real key");
    }

    #[test]
    fn a_paired_key_is_not_slandered_as_a_dev_key() {
        // The mirror image, and the reason provenance is recorded at all: once
        // real pairing shipped, the blanket disclaimer became its own false
        // statement -- telling someone who compared six digits that they had
        // not. Wrong in the reassuring direction and wrong in the alarming
        // direction are the same bug.
        let a = assess(&facts());
        let c = find(&a, ComponentId::Transport);
        assert!(!c.detail.contains("开发密钥"), "a paired key is not a dev key: {}", c.detail);
        assert!(c.detail.contains("配对"), "should say it is paired: {}", c.detail);
    }

    // ---- locking ------------------------------------------------------------

    #[test]
    fn a_mac_that_cannot_lock_is_told_so_rather_than_blanked() {
        // The drill exists to prove macOS really consults the plugin. With a
        // non-zero lock delay the session stays unlocked through the grace
        // period, so the screen would go dark, the user would press a key, and
        // nothing would have been tested -- while the panel said it had run.
        assert!(lock_readiness(Some("screenLock delay is immediate")).is_ok());
        assert!(lock_readiness(Some("screenLock delay is 300 seconds")).is_err());
        assert!(lock_readiness(Some("screenLock is off")).is_err());
    }

    #[test]
    fn an_unreadable_lock_setting_never_reads_as_ready() {
        // The one that matters: output we did not anticipate. Guessing "ok"
        // here would put the drill back to blanking the screen and calling it
        // a test.
        assert!(lock_readiness(None).is_err());
        assert!(lock_readiness(Some("")).is_err());
        assert!(lock_readiness(Some("some future wording")).is_err());
    }

    #[test]
    fn the_switch_reflects_something_it_can_change() {
        // It used to read `state`, which stays AwaitingVerification whether or
        // not the monitor runs -- so the switch was permanently ON, only its
        // turn-OFF branch was reachable, and clicking it did nothing visible.
        let mut f = facts();
        f.presence = PresenceReport::NotRunning;
        assert!(!assess(&f).presence_running, "a stopped monitor must read as off");
        f.presence = PresenceReport::NeverRan;
        assert!(!assess(&f).presence_running);
        f.presence = PresenceReport::Fresh { state: "away".into(), rssi: None };
        assert!(assess(&f).presence_running, "running and not seeing the phone is still running");
        // Something IS running here -- the scanner. Calling it off would offer
        // a switch to start a thing that is already started.
        f.presence = PresenceReport::NoAuthorization;
        assert!(assess(&f).presence_running);
    }

    #[test]
    fn a_wrong_length_key_is_no_key_at_all() {
        // The observed failure: a three-byte file, root-owned and 0600, written
        // by a printf bug. Permissions were perfect and the contents were
        // rubbish. assess() cannot read the bytes -- the file is root-only --
        // so the length is the check, and Missing is the only honest answer.
        //
        // Missing rather than a new Corrupt state on purpose: the remedy is
        // identical (pair again), and a state whose only button is the one
        // Missing already has is a state that exists to be rendered, not used.
        let mut f = facts();
        f.presence_key = PresenceKeyState::Missing;
        let a = assess(&f);
        assert_eq!(a.state, UnlockState::AwaitingPairing);
        let c = find(&a, ComponentId::Transport);
        assert!(c.remediation.is_some(), "must offer a way out");
    }

    #[test]
    fn an_installed_mac_with_no_key_is_sent_to_pair_not_to_a_drill() {
        // AwaitingVerification's button is 「锁屏，试一次」. With no key the
        // plugin can only deny, so that button invited the user to watch
        // nothing happen -- while the state that offers 配对手机 was never
        // returned by anything.
        let mut f = facts();
        f.presence_key = PresenceKeyState::Missing;
        assert_eq!(assess(&f).state, UnlockState::AwaitingPairing);
    }

    // ---- pairing stages ----------------------------------------------------
    //
    // The panel drives a real key exchange now, so the stage machine is the
    // thing standing between a person and a key derived from a conversation
    // they never checked. These assert that no path reaches a comparison the
    // tool did not offer, and that no failure is worded as a retry when it is
    // evidence of somebody in the middle.

    #[test]
    fn still_running_with_no_digits_is_scanning() {
        assert_eq!(pairing_stage(None, None).stage, PairingStage::Scanning);
    }

    #[test]
    fn digits_while_alive_are_the_comparison() {
        let s = pairing_stage(Some("063529\n"), None);
        assert_eq!(s.stage, PairingStage::Compare);
        assert_eq!(s.digits.as_deref(), Some("063529"));
    }

    #[test]
    fn a_tool_that_exited_before_digits_never_offers_a_comparison() {
        let s = pairing_stage(None, Some(Some(3)));
        assert_eq!(s.stage, PairingStage::Failed);
        assert!(s.digits.is_none());
    }

    #[test]
    fn digits_left_behind_by_a_dead_tool_are_not_still_answerable() {
        // The window timed out while the digits were on screen. Keeping
        // Compare would show live 一样/不一样 buttons wired to a process that
        // is gone -- the answer would vanish and the screen would sit there.
        let s = pairing_stage(Some("063529"), Some(Some(7)));
        assert_eq!(s.stage, PairingStage::Failed);
    }

    #[test]
    fn a_commitment_mismatch_is_never_described_as_worth_retrying() {
        // Exit 5 means the phone's revealed nonce did not match its earlier
        // commitment. That is what a man in the middle leaves behind, and
        // "try again" would walk the user straight back into it.
        let m = pairing_failure(Some(5));
        assert!(m.contains("冒充"), "must name what it means: {m}");
        assert!(!m.contains("再试一次"), "must not invite a retry: {m}");
    }

    #[test]
    fn every_failure_says_no_key_was_written_or_what_to_do() {
        for code in [Some(2), Some(3), Some(4), Some(6), Some(7), None] {
            let m = pairing_failure(code);
            assert!(!m.is_empty(), "code {code:?} has no explanation");
            assert!(
                m.contains("没有写入") || m.contains("再试一次") || m.contains("重新"),
                "code {code:?} leaves the user with no next step: {m}"
            );
        }
    }

    // ---- pipeline lifecycle ------------------------------------------------

    #[test]
    fn a_live_but_silent_pipeline_is_restarted_rather_than_left_wedged() {
        // The observed deadlock. The panel reads the status file to decide what
        // the switch shows; when the bridge stopped publishing (it only beat
        // while the phone was present) the switch went OFF on a pipeline that
        // was running fine. Clicking ON asked the backend, which saw a live pid
        // and answered Nothing. The screen said stopped, the backend said
        // running, and the control did neither -- every time the user walked
        // away from their desk.
        assert_eq!(
            pipeline_action(Some("4242"), true, true, false),
            PipelineAction::Restart(4242),
        );
        // Publishing and asked to run: genuinely nothing to do. Starting a
        // second would put two scanners on one radio.
        assert_eq!(
            pipeline_action(Some("4242"), true, true, true),
            PipelineAction::Nothing,
        );
        // Silence is irrelevant when the answer is "stop".
        assert_eq!(
            pipeline_action(Some("4242"), true, false, false),
            PipelineAction::Stop(4242),
        );
    }

    #[test]
    fn revoking_takes_the_provenance_with_the_key() {
        // Leaving `.provenance` behind would make the NEXT key -- which may well
        // be a dev key pushed over USB -- read as "paired by SAS". The panel
        // would then vouch for a key nobody verified.
        let script = revoke_script("/var/db/repose-unlock/presence-key.1");
        assert!(script.contains("presence-key.1'"), "the key itself: {script}");
        assert!(script.contains("presence-key.1'.provenance"), "and its provenance: {script}");
        assert!(script.contains("presence-key.1'.phone"), "and which phone it was: {script}");
        assert!(script.contains("with administrator privileges"));
    }

    #[test]
    fn a_quote_in_a_path_stays_one_shell_word() {
        // Not a grep for scary substrings -- "rm -rf /" appears verbatim inside
        // correctly quoted output and is inert there. The property that matters
        // is that /bin/sh hands the word back unchanged.
        //
        // Only the shell layer is covered. The AppleScript string around it
        // escapes nothing, so a path containing a double quote or a backslash
        // would still break the literal; every path passed here today is a
        // compile-time constant, and that is the reason it is safe rather than
        // an accident to rely on.
        let nasty = "/tmp/a'; rm -rf /; echo '";
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &format!("printf %s {}", applescript_quote(nasty))])
            .output()
            .expect("sh should run");
        assert_eq!(String::from_utf8_lossy(&out.stdout), nasty);
    }

    #[test]
    fn the_pipeline_does_not_require_slot_one() {
        // It checked presence-key.1 and nothing else. With pair-v3 the phone
        // picks its own slot, so a Mac whose only key was in slot 15 failed to
        // start the monitor -- switch off, no administrator prompt, no reason
        // on screen. Found by deleting the pre-v3 key after re-pairing.
        let sh = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tools/ble-spike/mac/presence-pipeline.sh"),
        )
        .expect("presence-pipeline.sh should be readable");
        assert!(
            !sh.contains("presence-key.${KEY_ID}"),
            "the pipeline still gates on one hardcoded slot",
        );
        assert!(
            sh.contains("presence-key.[0-9]*"),
            "the pipeline should accept a key in any slot",
        );
    }

    #[test]
    fn the_pipeline_honours_the_run_flag_we_name() {
        // start_pipeline picks a per-run flag name and passes it as
        // REPOSE_RUNFLAG. If the script ignored it and kept its own fixed
        // path, every restart would go back to racing the outgoing pipeline's
        // teardown -- which is how a successful pairing ended with rssi-scan
        // alive and nothing verifying (2026-09-12).
        let sh = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tools/ble-spike/mac/presence-pipeline.sh"),
        )
        .expect("presence-pipeline.sh should be readable");
        assert!(
            sh.contains("RUNFLAG=\"${REPOSE_RUNFLAG:-"),
            "presence-pipeline.sh ignores REPOSE_RUNFLAG, so naming it here does nothing",
        );

        let rs = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/unlock.rs"),
        )
        .expect("own source should be readable");
        assert!(
            rs.contains(".env(\"REPOSE_RUNFLAG\", &runflag)"),
            "the pipeline is spawned without being told which flag to use",
        );
    }

    /// Deterministic, symmetric around `mean`, so sd is a known quantity rather
    /// than something a random generator decides per run. One reading a second,
    /// which is roughly what the scanner produces when it is behaving.
    fn leg(mean: i32, spread: i32, n: usize) -> Vec<(i32, i64)> {
        (0..n)
            .map(|i| (mean + if i % 2 == 0 { spread } else { -spread }, 1_000 + i as i64 * 1_000))
            .collect()
    }

    /// Every reading at the same instant: what the scanner actually emits when
    /// it bursts near-duplicate rows.
    fn burst(mean: i32, spread: i32, n: usize) -> Vec<(i32, i64)> {
        (0..n)
            .map(|i| (mean + if i % 2 == 0 { spread } else { -spread }, 1_000 + i as i64))
            .collect()
    }

    #[test]
    fn bands_are_per_phone_and_sorted() {
        let raw = r#"{"byKey":{"15":{"nearDbm":-58,"farDbm":-79},"3":{"nearDbm":-60,"farDbm":-80}}}"#;
        assert_eq!(calibration_bands(raw), "15:-58:-79,3:-60:-80");
    }

    #[test]
    fn an_inverted_or_impossible_band_is_dropped_rather_than_passed_on() {
        // The file is plain JSON in the app's data directory. A band where near
        // is not above far tells the bridge one reading is both present and
        // absent; a slot 0 is not a slot. Either one silently breaks a phone
        // that was working, so neither reaches the bridge.
        for raw in [
            r#"{"byKey":{"15":{"nearDbm":-79,"farDbm":-58}}}"#,
            r#"{"byKey":{"15":{"nearDbm":-58,"farDbm":-58}}}"#,
            r#"{"byKey":{"0":{"nearDbm":-58,"farDbm":-79}}}"#,
            r#"{"byKey":{"abc":{"nearDbm":-58,"farDbm":-79}}}"#,
            r#"{"byKey":{"15":{"nearDbm":-58}}}"#,
            "not json at all",
            "{}",
        ] {
            assert_eq!(calibration_bands(raw), "", "accepted: {raw}");
        }
    }

    #[test]
    fn a_bad_band_beside_a_good_one_does_not_take_it_down() {
        let raw = r#"{"byKey":{"15":{"nearDbm":-58,"farDbm":-79},"3":{"nearDbm":-80,"farDbm":-60}}}"#;
        assert_eq!(calibration_bands(raw), "15:-58:-79");
    }

    #[test]
    fn the_bridge_reads_a_band_per_key() {
        let sh = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tools/ble-spike/mac/permit-bridge.sh"),
        )
        .expect("permit-bridge.sh should be readable");
        assert!(sh.contains("band_for"), "the bridge has no per-key band lookup");
        assert!(sh.contains("REPOSE_BANDS"), "the bridge never reads the bands");
    }

    #[test]
    fn the_calibrated_band_reaches_the_bridge() {
        // Before this, permit-bridge.sh read REPOSE_NEAR_DBM/REPOSE_FAR_DBM and
        // presence-pipeline.sh never set them, so the placeholders the bridge
        // itself calls "do not ship these numbers" were the only values any Mac
        // ever used. A calibration that computes thresholds nothing can read is
        // a longer way of shipping the placeholder.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/ble-spike/mac");
        let pipeline = std::fs::read_to_string(dir.join("presence-pipeline.sh")).unwrap();
        let privileged = std::fs::read_to_string(dir.join("presence-privileged.sh")).unwrap();
        let bridge = std::fs::read_to_string(dir.join("permit-bridge.sh")).unwrap();
        let rust = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/unlock.rs"),
        )
        .unwrap();

        // The FIRST version of this test asserted only that unlock.rs mentioned
        // `.env("REPOSE_NEAR_DBM"...)` somewhere. It did -- on the unprivileged
        // pipeline child -- and the bridge went on logging near>=-72 far<=-85
        // with the test green, because in local mode the privileged half is
        // started by the AppleScript below, not by that child.
        //
        // So the assertion is now about the string that actually starts it.
        let admin = rust
            .split("do shell script")
            .find(|chunk| chunk.contains("presence-privileged.sh") || chunk.contains("{priv_sh}"))
            .expect("the privileged half is started by an administrator script");
        for var in ["REPOSE_BANDS"] {
            assert!(
                admin.contains(var),
                "the administrator script that starts the privileged half omits {var}, \
                 so a calibrated band can never reach the bridge",
            );
            assert!(
                rust.contains(&format!(".env(\"{var}\"")),
                "the pipeline child is never told {var} either",
            );
            assert!(pipeline.contains(var), "presence-pipeline.sh drops {var} on the floor");
            assert!(privileged.contains(var), "presence-privileged.sh drops {var} on the floor");
            assert!(bridge.contains(var), "permit-bridge.sh does not read {var}");
        }
    }

    #[test]
    fn an_edited_calibration_file_cannot_invert_the_band() {
        // calibrated_thresholds refuses near <= far. The file is plain JSON in
        // the app's data directory; a typo there must not hand the bridge a
        // band where present and absent are both true.
        let ok = serde_json::json!({ "nearDbm": -55, "farDbm": -75 });
        let bad = serde_json::json!({ "nearDbm": -75, "farDbm": -55 });
        let read = |v: &serde_json::Value| -> Option<(i32, i32)> {
            let n = v.get("nearDbm")?.as_i64()? as i32;
            let f = v.get("farDbm")?.as_i64()? as i32;
            (n > f).then_some((n, f))
        };
        assert_eq!(read(&ok), Some((-55, -75)));
        assert_eq!(read(&bad), None);
    }

    #[test]
    fn an_empty_scanner_log_past_the_grace_is_news_not_silence() {
        // The live case: a Bluetooth dialog behind the window, CoreBluetooth
        // never delivering a state, and a panel saying it was watching.
        assert_eq!(radio_state(Some(""), RADIO_GRACE_S + 1), RadioState::NoAnswer);
        assert_eq!(radio_state(None, RADIO_GRACE_S + 1), RadioState::NoAnswer);
    }

    #[test]
    fn a_scanner_that_only_just_started_is_not_accused_of_anything() {
        // Alarming a second after start would make the everyday case -- turning
        // the switch on -- flash a warning that resolves itself, which is how a
        // warning stops being read.
        assert_eq!(radio_state(Some(""), 1), RadioState::Starting);
    }

    #[test]
    fn the_scanners_own_words_beat_the_clock() {
        assert_eq!(radio_state(Some("[t] STATE poweredOn\n"), 0), RadioState::Scanning);
        assert_eq!(radio_state(Some("[t] STATE unauthorized — grant\n"), 99), RadioState::Denied);
        assert_eq!(radio_state(Some("[t] STATE poweredOff — Bluetooth is off\n"), 99), RadioState::Off);
        assert_eq!(radio_state(Some("[t] STATE unsupported — no BLE radio\n"), 99), RadioState::Unsupported);
    }

    #[test]
    fn a_radio_switched_off_and_on_again_reads_as_on() {
        // Both words are in the log; the last one is the true one. Reading the
        // first would leave 「蓝牙关着」 on screen for a working Mac.
        let log = "[a] STATE poweredOff — Bluetooth is off\n[b] STATE poweredOn\n";
        assert_eq!(radio_state(Some(log), 99), RadioState::Scanning);
    }

    #[test]
    fn the_mac_id_comes_from_the_verifier_not_from_a_second_guess() {
        let csv = "\
1000,-53,ABC,2,1,tag,0,1,auth=VALID,cmd=0
macstate,1,59638225,e44241038ca4364f,d006c92720e9d1ce,ab12
1001,-55,ABC,2,1,tag,0,2,auth=VALID,cmd=0
";
        assert_eq!(mac_identity(csv), Some("AB12".to_string()));
    }

    #[test]
    fn the_newest_macstate_line_wins() {
        // The file is appended to for the life of the pipeline, so an old id
        // from before a hardware change must not outrank the current one.
        let csv = "\
macstate,1,59638000,aa,bb,0001
macstate,1,59638225,cc,dd,ab12
";
        assert_eq!(mac_identity(csv), Some("AB12".to_string()));
    }

    #[test]
    fn a_v1_macstate_line_yields_no_id_rather_than_a_wrong_one() {
        // Five fields is the old format, which had no id at all. Reading its
        // last field would report the locked tag's first four characters as
        // this Mac's identity.
        let csv = "macstate,1,59638225,e44241038ca4364f,d006c92720e9d1ce\n";
        assert_eq!(mac_identity(csv), None);
    }

    #[test]
    fn two_clear_clouds_give_a_band_with_a_gap_in_it() {
        let r = calibration_verdict(&leg(-50, 3, 30), &leg(-80, 3, 30));
        match r.outcome {
            CalibrationOutcome::Ok { near_dbm, far_dbm, far_silent } => {
                assert_eq!(near_dbm, -53, "NEAR is one sd below the near mean");
                assert_eq!(far_dbm, -77, "FAR is one sd above the far mean");
                assert!(near_dbm > far_dbm, "the band must not be inverted");
                assert!(!far_silent, "a measured far leg is not a silent one");
            }
            other => panic!("two clouds 30 dB apart should be separable: {other:?}"),
        }
        assert_eq!(r.near.n, 30);
        assert_eq!(r.far.mean, -80.0);
        assert_eq!(r.near.span_ms, 29_000);
    }

    #[test]
    fn a_silent_far_leg_is_the_clearest_answer_not_a_failure() {
        // The person walked far enough that the Mac stopped hearing the phone.
        // That is not a measurement that failed; it is the best separation
        // there is. FAR sits one full gap below NEAR, and the bridge's own
        // no-beacon rule covers the rest.
        for far in [Vec::new(), leg(-88, 2, 5), burst(-80, 3, 30)] {
            let r = calibration_verdict(&leg(-50, 3, 30), &far);
            assert_eq!(
                r.outcome,
                CalibrationOutcome::Ok { near_dbm: -53, far_dbm: -65, far_silent: true },
                "far leg with {} readings", far.len(),
            );
        }
    }

    #[test]
    fn a_silent_near_leg_is_still_a_failure() {
        // Silence where the person is sitting means the Mac cannot hear the
        // phone at all. There is no threshold to derive from nothing.
        let r = calibration_verdict(&leg(-50, 3, 5), &Vec::new());
        assert!(matches!(r.outcome, CalibrationOutcome::NotEnoughSamples { near: 5, .. }), "{:?}", r.outcome);
        let r = calibration_verdict(&burst(-50, 3, 30), &leg(-80, 3, 30));
        assert!(matches!(r.outcome, CalibrationOutcome::TooBrief { .. }), "{:?}", r.outcome);
    }

    #[test]
    fn the_outcome_names_the_silent_far_leg_for_the_panel() {
        let json = serde_json::to_string(&CalibrationOutcome::Ok { near_dbm: -53, far_dbm: -65, far_silent: true })
            .unwrap();
        assert!(json.contains(r#""kind":"ok""#), "{json}");
        assert!(json.contains(r#""farSilent":true"#), "{json}");
        // The front end reads camelCase on every variant (src/lib/unlock.ts
        // normalizeCalibration); snake_case here read as 0 dBm over there.
        assert!(json.contains(r#""nearDbm":-53"#), "{json}");
        let brief = serde_json::to_string(&CalibrationOutcome::TooBrief { near_ms: 1, far_ms: 2, needed_ms: 3 }).unwrap();
        assert!(brief.contains(r#""kind":"too-brief""#) && brief.contains(r#""neededMs":3"#), "{brief}");
    }

    #[test]
    fn twenty_readings_of_one_instant_are_not_twenty_seconds_of_evidence() {
        // The first live run filled the progress bar in under three seconds:
        // rssi-scan emits near-duplicate rows a millisecond apart, so the count
        // raced ahead of any actual observation. A standard deviation from that
        // describes the scanner's loop, not the room.
        let r = calibration_verdict(&burst(-50, 3, 30), &burst(-80, 3, 30));
        match r.outcome {
            CalibrationOutcome::TooBrief { needed_ms, .. } => {
                assert_eq!(needed_ms, CALIBRATION_MIN_SPAN_MS)
            }
            other => panic!("a 29 ms leg must not produce thresholds: {other:?}"),
        }
    }

    #[test]
    fn a_short_walk_is_refused_rather_than_averaged() {
        // The whole point of calibration is to stop guessing. Nineteen samples
        // and a confident answer is a guess wearing a number.
        let r = calibration_verdict(&leg(-50, 3, 19), &leg(-80, 3, 30));
        assert_eq!(
            r.outcome,
            CalibrationOutcome::NotEnoughSamples { near: 19, far: 30, needed: CALIBRATION_MIN_SAMPLES }
        );
    }

    #[test]
    fn a_phone_in_a_bag_two_metres_away_fails_honestly() {
        // ui-conventions 1.3: a calibration that cannot fail writes down a
        // number it does not believe. Near and far only 6 dB apart is the
        // radio saying it cannot tell this desk from that doorway.
        let r = calibration_verdict(&leg(-70, 2, 30), &leg(-76, 2, 30));
        match r.outcome {
            CalibrationOutcome::TooSimilar { gap_db, needed_db } => {
                assert!((gap_db - 6.0).abs() < 0.001);
                assert_eq!(needed_db, CALIBRATION_MIN_GAP_DB);
            }
            other => panic!("6 dB apart must not produce thresholds: {other:?}"),
        }
    }

    #[test]
    fn two_noisy_clouds_that_overlap_are_refused_even_when_their_means_are_far() {
        // Means 14 dB apart passes the gap test, but +-8 dB of spread on each
        // leg pushes FAR above NEAR. The band would be inverted, and the bridge
        // would read present and absent from the same sample.
        let r = calibration_verdict(&leg(-52, 8, 30), &leg(-66, 8, 30));
        assert!(
            matches!(r.outcome, CalibrationOutcome::TooSimilar { .. }),
            "overlapping spreads must not produce an inverted band: {:?}",
            r.outcome,
        );
    }

    // ---- calibration driven from the phone --------------------------------

    #[test]
    fn a_phase_line_round_trips_and_expires() {
        assert_eq!(parse_phase(&phase_line(PHASE_WAIT, 0), 5_000), Some(PHASE_WAIT));
        assert_eq!(parse_phase(&phase_line(PHASE_OK, 10_000), 9_999), Some(PHASE_OK));
        // Expired: the Mac goes back to saying whether it is locked.
        assert_eq!(parse_phase(&phase_line(PHASE_OK, 10_000), 10_000), None);
    }

    #[test]
    fn an_unreadable_phase_falls_back_to_the_lock_state_not_to_a_guess() {
        assert_eq!(parse_phase("", 0), None);
        assert_eq!(parse_phase("garbage", 0), None);
        assert_eq!(parse_phase("5", 0), None);
        // Lock states are not phases: the file must not be able to claim 「锁着」.
        assert_eq!(parse_phase("1 0", 0), None);
        assert_eq!(parse_phase("9 0", 0), None);
    }

    #[test]
    fn a_leg_runs_for_twenty_seconds_full_stop() {
        // The phone shows the same twenty-second countdown. A leg that ended
        // early because it had "enough" would leave the person sitting still
        // for a countdown that no longer measures anything; one that ran long
        // would end after they had already got up.
        assert_eq!(CALIBRATION_LEG_MS, 20_000);
        assert!(!calibration_leg_done(0));
        assert!(!calibration_leg_done(CALIBRATION_LEG_MS - 1));
        assert!(calibration_leg_done(CALIBRATION_LEG_MS));
        assert!(calibration_leg_done(CALIBRATION_LEG_MS + 5_000));
    }

    #[test]
    fn the_verdict_becomes_the_beacon_value_the_phone_screen_keys_on() {
        assert_eq!(phase_for_outcome(&CalibrationOutcome::Ok { near_dbm: -60, far_dbm: -80, far_silent: false }), PHASE_OK);
        // Silence at the far spot is a verdict the phone shows as 量好了.
        assert_eq!(phase_for_outcome(&CalibrationOutcome::Ok { near_dbm: -60, far_dbm: -72, far_silent: true }), PHASE_OK);
        assert_eq!(phase_for_outcome(&CalibrationOutcome::TooSimilar { gap_db: 3.0, needed_db: 12.0 }), PHASE_FAIL);
        assert_eq!(phase_for_outcome(&CalibrationOutcome::NotEnoughSamples { near: 2, far: 0, needed: 20 }), PHASE_SILENT);
        assert_eq!(phase_for_outcome(&CalibrationOutcome::TooBrief { near_ms: 100, far_ms: 0, needed_ms: 10_000 }), PHASE_SILENT);
    }

    #[test]
    fn each_calibration_command_is_only_in_turn_at_its_own_moment() {
        // 4 near: any time except while a near leg is being walked (再量一次).
        assert!(calibration_in_turn(0, CAL_CMD_NEAR));
        assert!(calibration_in_turn(PHASE_WAIT, CAL_CMD_NEAR));
        assert!(calibration_in_turn(PHASE_FAR, CAL_CMD_NEAR));
        assert!(!calibration_in_turn(PHASE_NEAR, CAL_CMD_NEAR));
        // 5 far: only while the near leg is waiting for the walk.
        assert!(calibration_in_turn(PHASE_WAIT, CAL_CMD_FAR));
        assert!(!calibration_in_turn(0, CAL_CMD_FAR));
        assert!(!calibration_in_turn(PHASE_NEAR, CAL_CMD_FAR));
        assert!(!calibration_in_turn(PHASE_FAR, CAL_CMD_FAR));
        // 6 far done: while waiting (the far command never arrived: the far
        // spot is out of range) or while the far leg is being sampled (cut it
        // short and judge). Never before a near leg, never after a verdict.
        assert!(calibration_in_turn(PHASE_WAIT, CAL_CMD_FAR_DONE));
        assert!(calibration_in_turn(PHASE_FAR, CAL_CMD_FAR_DONE));
        assert!(!calibration_in_turn(0, CAL_CMD_FAR_DONE));
        assert!(!calibration_in_turn(PHASE_NEAR, CAL_CMD_FAR_DONE));
        // Not a calibration byte at all.
        assert!(!calibration_in_turn(PHASE_WAIT, 16));
        assert!(!calibration_in_turn(PHASE_WAIT, 0));
    }

    #[test]
    fn calibration_commands_are_read_off_the_same_stream_as_the_shortcuts() {
        let csv = "\
1000,rssi=-50,auth=VALID,x,166,cmd=4,seq=1,a,b\n\
1500,rssi=-50,auth=NOKEY,x,166,cmd=4,seq=2,a,b\n\
2000,rssi=-50,auth=VALID,x,166,cmd=16,seq=3,a,b\n\
2500,rssi=-50,auth=VALID,x,17,cmd=5,seq=4,a,b\n\
3000,rssi=-50,auth=VALID,x,17,cmd=6,seq=5,a,b\n\
3500,rssi=-50,auth=VALID,x,17,cmd=7,seq=6,a,b\n";
        assert_eq!(calibration_commands_since(csv, 0), vec![(4, 1000, 166), (5, 2500, 17), (6, 3000, 17)]);
        assert_eq!(calibration_commands_since(csv, 2500), vec![(6, 3000, 17)]);
    }

    #[test]
    fn walking_the_legs_backwards_fails_instead_of_inverting_the_band() {
        // Someone who walks away first and then stands still, without telling
        // the app. A negative gap must not be turned into thresholds by
        // sign-blind arithmetic.
        let r = calibration_verdict(&leg(-80, 3, 30), &leg(-50, 3, 30));
        assert!(matches!(r.outcome, CalibrationOutcome::TooSimilar { gap_db, .. } if gap_db < 0.0));
    }

    #[test]
    fn a_macstate_row_is_not_a_signal_reading() {
        // verified.csv carries `macstate,<keyId>,<counter>,<tag>,<tag>` rows for
        // the advertiser. Reading column 1 blindly would turn the key id into a
        // -1 dBm sample -- a phone touching the antenna, at the exact moment
        // we are deciding what "near" means.
        let csv = "\
1000,-53,ABC,2,1,tag,0,1,auth=VALID,cmd=0
macstate,1,59638225,e44241038ca4364f,d006c92720e9d1ce
1001,-55,ABC,2,1,tag,0,2,auth=VALID,cmd=0
";
        assert_eq!(calibration_samples(csv, 0, 1), vec![(-53, 1000), (-55, 1001)]);
    }

    #[test]
    fn samples_from_before_this_leg_started_do_not_count() {
        let csv = "\
1000,-90,ABC,2,1,tag,0,1,auth=VALID,cmd=0
2000,-53,ABC,2,1,tag,0,2,auth=VALID,cmd=0
";
        assert_eq!(calibration_samples(csv, 1500, 1), vec![(-53, 2000)]);
    }

    #[test]
    fn a_rejected_beacon_is_not_evidence_about_distance() {
        // auth=BAD means something was transmitting on our service id that we
        // could not authenticate. Letting it into the near cloud would let a
        // stranger's radio move this Mac's idea of where its owner sits.
        let csv = "\
1000,-40,ABC,2,1,tag,0,1,auth=BAD,cmd=0
1001,-53,ABC,2,1,tag,0,2,auth=VALID,cmd=0
";
        assert_eq!(calibration_samples(csv, 0, 1), vec![(-53, 1001)]);
    }

    #[test]
    fn another_phones_key_slot_is_not_this_phones_signal() {
        let csv = "\
1000,-40,ABC,2,7,tag,0,1,auth=VALID,cmd=0
1001,-53,ABC,2,1,tag,0,2,auth=VALID,cmd=0
";
        assert_eq!(calibration_samples(csv, 0, 1), vec![(-53, 1001)]);
    }

    fn slot(id: u8, state: PresenceKeyState) -> KeySlot {
        KeySlot { id, identified: true, state, written_at: None }
    }

    #[test]
    fn the_pairing_tools_line_is_parsed_strictly() {
        let key = "a".repeat(64);
        assert_eq!(
            parse_pair_output(&format!("37 0badc0de0badc0de {key} {ck}\n", ck = "b".repeat(64))),
            Some((37, "0badc0de0badc0de".to_string(), key.clone(), "b".repeat(64))),
        );
        // Uppercase from either side means the same phone.
        assert_eq!(
            parse_pair_output(&format!("37 0BADC0DE0BADC0DE {key} {ck}", ck = "b".repeat(64)))
                .unwrap()
                .1,
            "0badc0de0badc0de",
        );
    }

    #[test]
    fn a_malformed_line_fails_the_pairing_rather_than_installing_half_of_it() {
        // Every one of these, read loosely, installs something the verifier
        // will refuse while the panel says 配对完成 -- which is exactly how the
        // printf bug reached a lock screen.
        let key = "a".repeat(64);
        let ck = "b".repeat(64);
        for bad in [
            "".to_string(),
            key.clone(),                                        // the old one-field format
            format!("37 0badc0de0badc0de {key}"),               // the v3 form without the console key
            format!("0 0badc0de0badc0de {key} {ck}"),           // slot 0 is not a slot
            format!("37 0badc0de {key} {ck}"),                  // short identity
            format!("37 0badc0de0badc0dez {key} {ck}"),         // not hex
            format!("37 0badc0de0badc0de {} {ck}", "a".repeat(63)),
            format!("37 0badc0de0badc0de {key} {}", "b".repeat(63)),
            format!("37 0badc0de0badc0de {key} {ck} extra"),    // a field we do not understand
            format!("999 0badc0de0badc0de {key} {ck}"),         // out of range
        ] {
            assert!(parse_pair_output(&bad).is_none(), "accepted: {bad:?}");
        }
    }

    #[test]
    fn pairing_is_consent_to_unlock_but_not_consent_to_type() {
        // The two defaults are deliberately opposite. A phone that had to be
        // switched on after pairing would look like a pairing that had not
        // finished; a phone that could type into whatever is open the moment it
        // paired would be a capability nobody asked for.
        let caps = DeviceCapabilities::default();
        assert!(caps.unlock_allowed(15));
        assert!(!caps.control_allowed(15));
    }

    #[test]
    fn a_switched_off_phone_says_which_switch_is_off() {
        // With both off, telling someone to go flip the master switch sends them
        // to the wrong place.
        let caps = DeviceCapabilities { unlock_off: vec![15], control_on: vec![] };
        let rows = paired_devices(
            &[slot(15, PresenceKeyState::Ok { paired: true })],
            &|_| None,
            true,
            &caps,
        );
        assert!(!rows[0].can_unlock);
        assert!(!rows[0].unlock_allowed);
        assert!(rows[0].blocked_reason.as_deref().unwrap().contains("这部手机"));

        // Allowed, but nothing is watching: a different sentence.
        let rows = paired_devices(
            &[slot(15, PresenceKeyState::Ok { paired: true })],
            &|_| None,
            false,
            &DeviceCapabilities::default(),
        );
        assert!(rows[0].unlock_allowed, "the decision survives the monitor being off");
        assert!(!rows[0].can_unlock);
        assert!(rows[0].blocked_reason.as_deref().unwrap().contains("总开关"));
    }

    #[test]
    fn switching_one_phone_off_leaves_the_others_alone() {
        let caps = DeviceCapabilities { unlock_off: vec![15], control_on: vec![9] };
        let rows = paired_devices(
            &[
                slot(9, PresenceKeyState::Ok { paired: true }),
                slot(15, PresenceKeyState::Ok { paired: true }),
            ],
            &|_| None,
            true,
            &caps,
        );
        assert!(rows[0].can_unlock && rows[0].control_allowed, "9 should be untouched");
        assert!(!rows[1].can_unlock && !rows[1].control_allowed);
    }

    #[test]
    fn an_unreadable_capability_file_means_the_safe_defaults() {
        for raw in ["", "not json", "{}", r#"{"unlockOff":"x","controlOn":5}"#] {
            let caps = parse_capabilities(raw);
            assert!(caps.unlock_allowed(15), "{raw}");
            assert!(!caps.control_allowed(15), "{raw}");
        }
        let caps = parse_capabilities(r#"{"unlockOff":[15,999],"controlOn":[9]}"#);
        assert!(!caps.unlock_allowed(15));
        // 999 is not a slot; dropping it is right, and keeping it would have
        // meant a file that cannot be parsed disabling nothing at all.
        assert!(caps.unlock_allowed(9));
        assert!(caps.control_allowed(9));
    }

    #[test]
    fn the_bridge_reads_the_off_switch_while_it_runs() {
        // Taken once at start, it would need an administrator password and a
        // pipeline restart to take effect -- and a switch that costs a password
        // is a switch nobody flips.
        let sh = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tools/ble-spike/mac/permit-bridge.sh"),
        )
        .expect("permit-bridge.sh should be readable");
        assert!(sh.contains("refresh_disabled"), "the bridge never re-reads the list");
        assert!(sh.contains("REPOSE_DISABLED_FILE"), "the bridge never reads the list at all");
        // A switched-off phone must not still be able to lock the Mac.
        assert!(
            sh.contains(r#"[ "${disabled}" = 1 ] || run_command lock"#),
            "a switched-off phone can still send commands",
        );
    }

    #[test]
    fn nothing_that_can_ask_for_a_password_runs_on_the_main_thread() {
        // A synchronous Tauri command runs on the main thread, and the
        // administrator dialog does not return until it is answered. The window
        // then freezes for the length of the dialog -- and permanently if the
        // dialog is dismissed unanswered or lands behind something, which is
        // how this Mac ended up with a live app, zero windows, and nothing to
        // do but kill it.
        //
        // Checked by reading our own source because there is no type that
        // distinguishes "can raise a prompt" from "cannot": the property lives
        // in what the function eventually calls.
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/unlock.rs"),
        )
        .expect("own source should be readable");

        // Commands whose body reaches run_privileged, directly or through the
        // backend. Listed rather than inferred: a wrong inference here would
        // pass while the window froze.
        for name in [
            "unlock_install",
            "unlock_repair",
            "unlock_uninstall",
            "unlock_revoke_device",
            "unlock_pair_confirm",
            "unlock_presence_set",
        ] {
            let sig = src
                .lines()
                .find(|l| l.contains(&format!("fn {name}(")))
                .unwrap_or_else(|| panic!("{name} not found"));
            assert!(
                sig.contains("pub async fn"),
                "{name} is synchronous, so its administrator prompt freezes the window: {sig}",
            );
        }
    }

    #[test]
    fn no_usable_key_means_an_empty_list() {
        // A device row is a promise that something can unlock this Mac. With no
        // key, or a key the verifier refuses, nothing can -- and a row saying
        // otherwise is the bug this whole file is written against.
        let names = |_: u8| Some("realme GT5 Pro".to_string());
        assert!(paired_devices(&[], &names, true, &DeviceCapabilities::default()).is_empty());
        assert!(paired_devices(&[slot(1, PresenceKeyState::Missing)], &names, true, &DeviceCapabilities::default()).is_empty());
        assert!(paired_devices(
            &[slot(1, PresenceKeyState::BadPermissions { detail: String::new() })],
            &names,
            true,
            &DeviceCapabilities::default(),
        )
        .is_empty());
    }

    #[test]
    fn every_slot_becomes_a_row_of_its_own() {
        // The point of the whole change: two phones are two rows, each with the
        // name recorded against ITS slot. One shared name would put the first
        // phone's label on the second one's key.
        let names = |id: u8| Some(format!("手机 {id}"));
        let rows = paired_devices(
            &[
                slot(7, PresenceKeyState::Ok { paired: true }),
                slot(1, PresenceKeyState::Ok { paired: true }),
            ],
            &names,
            true,
            &DeviceCapabilities::default(),
        );
        assert_eq!(rows.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["1", "7"]);
        assert_eq!(rows[0].name, "手机 1");
        assert_eq!(rows[1].name, "手机 7");
    }

    #[test]
    fn a_dev_key_is_listed_but_not_called_a_paired_phone() {
        // It really can unlock this Mac, so hiding it would be a lie by
        // omission; calling it a paired phone would be the opposite lie.
        let rows = paired_devices(
            &[slot(1, PresenceKeyState::Ok { paired: false })],
            &|_| Some("realme GT5 Pro".into()),
            true,
            &DeviceCapabilities::default(),
        );
        assert!(!rows[0].paired);
        assert!(!rows[0].name.contains("realme"), "a saved name must not label a dev key: {}", rows[0].name);
    }

    #[test]
    fn a_paired_phone_without_a_saved_name_still_gets_a_row() {
        for name in [None, Some(String::new()), Some("   ".to_string())] {
            let rows = paired_devices(&[slot(1, PresenceKeyState::Ok { paired: true })], &|_| name.clone(), true, &DeviceCapabilities::default());
            assert_eq!(rows[0].name, "已配对的手机");
        }
    }

    #[test]
    fn a_phone_that_cannot_unlock_says_why() {
        let rows = paired_devices(&[slot(1, PresenceKeyState::Ok { paired: true })], &|_| None, false, &DeviceCapabilities::default());
        assert!(!rows[0].can_unlock);
        assert!(rows[0].blocked_reason.is_some(), "a disabled row must carry its reason");
    }

    #[test]
    fn one_misowned_key_is_the_news_even_beside_good_ones() {
        // The verifier will refuse that key, so the phone it belongs to will
        // silently stop working while the panel reports 已配对 on the strength
        // of the other one.
        let state = aggregate_key_state(&[
            slot(1, PresenceKeyState::Ok { paired: true }),
            slot(9, PresenceKeyState::BadPermissions { detail: "x".into() }),
        ]);
        assert!(matches!(state, PresenceKeyState::BadPermissions { .. }));
    }

    #[test]
    fn the_panel_only_vouches_for_a_set_where_every_key_was_paired() {
        assert_eq!(
            aggregate_key_state(&[
                slot(1, PresenceKeyState::Ok { paired: true }),
                slot(2, PresenceKeyState::Ok { paired: false }),
            ]),
            PresenceKeyState::Ok { paired: false },
        );
        assert_eq!(
            aggregate_key_state(&[
                slot(1, PresenceKeyState::Ok { paired: true }),
                slot(2, PresenceKeyState::Ok { paired: true }),
            ]),
            PresenceKeyState::Ok { paired: true },
        );
        assert_eq!(aggregate_key_state(&[]), PresenceKeyState::Missing);
    }

    #[test]
    fn nothing_recorded_means_start() {
        assert_eq!(pipeline_action(None, false, true, true), PipelineAction::Start);
    }

    #[test]
    fn a_live_pipeline_is_not_started_again() {
        // Two scanners on one radio is not twice the presence; it is two
        // processes taking turns missing the phone.
        assert_eq!(pipeline_action(Some("4242"), true, true, true), PipelineAction::Nothing);
    }

    #[test]
    fn a_pidfile_left_by_a_crash_starts_rather_than_blocks() {
        // The file outlives the process. Treating a stale one as "already
        // running" would leave presence permanently off with no way back except
        // finding and deleting a file the user has never heard of.
        assert_eq!(pipeline_action(Some("4242"), false, true, true), PipelineAction::Start);
    }

    #[test]
    fn stopping_signals_the_recorded_pid() {
        assert_eq!(pipeline_action(Some("4242"), true, false, true), PipelineAction::Stop(4242));
    }

    #[test]
    fn stopping_something_already_gone_does_nothing() {
        assert_eq!(pipeline_action(Some("4242"), false, false, true), PipelineAction::Nothing);
        assert_eq!(pipeline_action(None, false, false, true), PipelineAction::Nothing);
    }

    #[test]
    fn a_nonsense_pidfile_is_not_a_pid() {
        // Including the two that would be catastrophic to signal.
        for bad in ["", "  ", "nope", "-1", "0", "1", "99999999999999999999"] {
            assert_eq!(read_pid(bad), None, "{bad:?} must not parse as a pid");
        }
        assert_eq!(read_pid(" 4242\n"), Some(4242));
    }

    #[test]
    fn a_nonsense_pidfile_starts_rather_than_signalling_something_random() {
        assert_eq!(pipeline_action(Some("nope"), true, true, true), PipelineAction::Start);
        assert_eq!(pipeline_action(Some("1"), true, false, true), PipelineAction::Nothing);
    }

    // ---- presence ---------------------------------------------------------

    // ---- foreign mechanisms in the lock-screen rule -----------------------

    #[test]
    fn an_untouched_mac_reports_no_foreign_mechanisms() {
        assert!(foreign_mechanisms(r#"["use-login-window-ui"]"#).is_empty());
        assert!(foreign_mechanisms(r#"["authenticate-session-owner-or-admin"]"#).is_empty());
    }

    #[test]
    fn finding_ourselves_is_not_foreign() {
        // A second install must not describe the first one as a stranger.
        assert!(foreign_mechanisms(r#"["ai.repose.spike","use-login-window-ui"]"#).is_empty());
    }

    #[test]
    fn a_third_party_plugin_is_named() {
        // The exact rule found on the development Mac on 2026-09-11, put here so
        // the case stays represented after that machine changes.
        let found = foreign_mechanisms(
            r#"["com.openai.sky.CUAService.AuthorizationPlugin.remote","use-login-window-ui"]"#,
        );
        assert_eq!(found, vec!["com.openai.sky.CUAService.AuthorizationPlugin.remote"]);
    }

    #[test]
    fn decide_variant_alone_would_have_approved_that_machine() {
        // Why the check above had to exist. decide_variant keys on the Apple
        // mechanism still being present, which it was, so preflight said yes to
        // a lock screen a stranger had already reconfigured.
        assert_eq!(
            decide_variant(r#"["com.openai.sky.CUAService.AuthorizationPlugin.remote","use-login-window-ui"]"#),
            Some(RuleVariant::A),
        );
    }

    #[test]
    fn an_unparseable_rule_reports_nothing_rather_than_guessing() {
        assert!(foreign_mechanisms("not json").is_empty());
    }

    #[test]
    fn each_component_appears_at_most_once() {
        // The first version pushed a second Transport row when the bridge was
        // not running, so the panel listed 配对密钥 twice, saying two different
        // things, and the reader had to guess which one counted.
        for f in [
            facts(),
            HostFacts { presence: PresenceReport::NeverRan, ..facts() },
            HostFacts { presence: PresenceReport::NotRunning, presence_key: PresenceKeyState::Missing, ..facts() },
            HostFacts { bundle_present: false, bundle_signature_ok: None, ..facts() },
        ] {
            let a = assess(&f);
            for id in ["Rule", "Component", "Daemon", "Transport"] {
                let n = a.components.iter().filter(|c| format!("{:?}", c.id) == id).count();
                assert!(n <= 1, "{id} appeared {n} times for {f:?}");
            }
        }
    }

    #[test]
    fn a_near_line_reads_as_near() {
        let a = assess(&facts());
        assert_eq!(a.presence, Presence::Near);
    }

    #[test]
    fn an_away_line_reads_as_away_and_adds_no_warning_row() {
        let a = assess(&HostFacts {
            presence: PresenceReport::Fresh { state: "away".into(), rssi: None },
            ..facts()
        });
        assert_eq!(a.presence, Presence::Away);
        // Away is the ordinary state of a phone in another room. It must not
        // also raise a degraded transport row, or every walk to the kitchen
        // looks like a malfunction.
        assert_eq!(find(&a, ComponentId::Transport).health, Health::Ok);
    }

    #[test]
    fn a_bridge_that_is_not_running_is_not_the_same_as_a_phone_that_left() {
        // Both end in the password path, but only one of them means the feature
        // is working. Showing "nothing is watching" as a calm Away is the whole
        // reason the status line carries a clock.
        for report in [PresenceReport::NeverRan, PresenceReport::NotRunning, PresenceReport::Unreadable] {
            let a = assess(&HostFacts { presence: report.clone(), ..facts() });
            assert_eq!(a.presence, Presence::TransportUnavailable, "{report:?}");
            let t = find(&a, ComponentId::Transport);
            assert_eq!(t.health, Health::Degraded, "{report:?}");
            assert!(t.detail.contains("密码照常能登录") || t.detail.contains("没有在运行"), "{report:?}: {}", t.detail);
        }
    }

    #[test]
    fn a_stale_line_stops_counting() {
        // The bridge republishes every few seconds, so a line from a minute ago
        // means it died -- not that the phone is still where it last was.
        let now = 1_800_000_000;
        assert_eq!(read_presence(Some("near,-55,1799999995"), now),
                   PresenceReport::Fresh { state: "near".into(), rssi: Some(-55) });
        assert_eq!(read_presence(Some("near,-55,1799999000"), now), PresenceReport::NotRunning);
    }

    #[test]
    fn a_line_from_the_future_is_refused_rather_than_trusted() {
        // A clock that jumped forward must not make a stale reading look fresh.
        let now = 1_800_000_000;
        assert_eq!(read_presence(Some("near,-55,1900000000"), now), PresenceReport::NotRunning);
    }

    #[test]
    fn a_stopped_bridge_says_so_even_with_a_fresh_stamp() {
        let now = 1_800_000_000;
        assert_eq!(read_presence(Some("stopped,-,1799999999"), now), PresenceReport::NotRunning);
    }

    #[test]
    fn garbage_never_reads_as_present() {
        let now = 1_800_000_000;
        for bad in ["", "near", "near,-55", "near,-55,notanumber"] {
            assert_ne!(
                read_presence(Some(bad), now),
                PresenceReport::Fresh { state: "near".into(), rssi: Some(-55) },
                "{bad:?} must not read as present",
            );
        }
        assert_eq!(read_presence(None, now), PresenceReport::NeverRan);
    }

    // ---- removal reports what it found --------------------------------

    fn clean() -> RemovalFacts {
        RemovalFacts {
            rule_now: r#"["use-login-window-ui"]"#.into(),
            bundle_present: false,
            key_present: false,
            permit_dir_present: false,
            support_dir_present: false,
        }
    }

    #[test]
    fn a_clean_removal_reports_nothing_left() {
        let r = uninstall_report("t".into(), &clean());
        assert!(r.right_removed && r.bundle_removed && r.keys_removed);
        assert!(r.residual.is_empty(), "{:?}", r.residual);
    }

    #[test]
    fn a_key_left_behind_is_never_reported_as_removed() {
        // The bug this function exists for. bundle_removed and keys_removed were
        // the literal `true`, so the removal screen told the user their presence
        // key was deleted while it sat in /var/db/repose-unlock. A shared secret
        // surviving "remove everything" is the leftover that matters, so it is
        // reported and named.
        let r = uninstall_report("t".into(), &RemovalFacts { key_present: true, ..clean() });
        assert!(!r.keys_removed);
        assert!(r.residual.iter().any(|x| x.contains("配对密钥")), "{:?}", r.residual);
    }

    #[test]
    fn a_surviving_bundle_is_reported() {
        let r = uninstall_report("t".into(), &RemovalFacts { bundle_present: true, ..clean() });
        assert!(!r.bundle_removed);
        assert!(!r.residual.is_empty());
    }

    #[test]
    fn a_rule_still_pointing_at_us_is_the_dangerous_leftover() {
        // Rule present, bundle gone: the fail-open state, created by our own
        // uninstaller. It must never read as a clean removal.
        let r = uninstall_report(
            "t".into(),
            &RemovalFacts { rule_now: r#"["ai.repose.spike","use-login-window-ui"]"#.into(), ..clean() },
        );
        assert!(!r.right_removed);
        assert!(!r.backup_used, "a restore that did not restore is not a restore");
        assert!(r.residual.iter().any(|x| x.contains(SUBRULE_NAME)));
    }

    #[test]
    fn leftovers_are_listed_all_at_once_not_one_at_a_time() {
        // Reporting only the first would send someone round the loop repeatedly.
        let r = uninstall_report(
            "t".into(),
            &RemovalFacts {
                bundle_present: true,
                key_present: true,
                permit_dir_present: true,
                support_dir_present: true,
                ..clean()
            },
        );
        assert_eq!(r.residual.len(), 4, "{:?}", r.residual);
    }

    #[test]
    fn the_uninstaller_script_removes_the_presence_key() {
        // It did not, for the whole time the app claimed it did. Asserted
        // against the script because that is where the deletion has to happen --
        // the Rust only reports what is left.
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../native/macos/minimal-auth-plugin/uninstall.sh"),
        )
        .expect("uninstall.sh should be readable");
        assert!(
            script.contains("presence-key"),
            "uninstall.sh does not remove the presence key",
        );
        assert!(script.contains(PRESENCE_KEY_DIR), "uninstall.sh does not touch {PRESENCE_KEY_DIR}");
    }

    #[test]
    fn everything_the_installer_needs_is_bundled() {
        // resolve_scripts_dir looks for install.sh inside the app's resources.
        // If a file the installer reaches for is not listed in tauri.conf.json it
        // is simply absent from the built app, and the only symptom is
        // "找不到安装脚本" on a user's machine -- nothing fails at build time,
        // and nothing fails in `tauri dev`, where the repo fallback path hides it.
        let conf = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"),
        )
        .expect("tauri.conf.json should be readable");
        for needed in [
            "install.sh",
            "uninstall.sh",
            "healthcheck.sh",
            "authdb-edit",
            "ai.repose.spike.healthcheck.plist",
            // install.sh copies this into place; without it the app installs a
            // rule pointing at a bundle that was never shipped -- the fail-open
            // state, created by our own installer.
            "ReposeSpike.bundle",
            // presence-pipeline.sh sources this. In the repo it is two levels up;
            // in the bundle it is one. Shipping it at all is the part a resource
            // map can silently drop.
            "run-root.sh",
        ] {
            assert!(conf.contains(needed), "tauri.conf.json bundles no {needed}");
        }
    }

    #[test]
    fn every_privileged_script_call_answers_the_script_s_own_prompt() {
        // install.sh and uninstall.sh both ask "Proceed? [y/N]" on a terminal.
        // `do shell script` gives them no stdin, so the read hits EOF and the
        // script aborts -- after the user has agreed in the app's disclosure and
        // typed their password. install was missing ASSUME_YES for exactly that
        // reason, and the generic failure message reported it as the user having
        // declined authorization.
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/unlock.rs"),
        )
        .expect("own source should be readable");
        for line in src.lines() {
            let l = line.trim();
            // Comments talk about this too, including the one explaining the bug.
            if l.starts_with("//") || l.starts_with("///") || !l.contains("do shell script") {
                continue;
            }
            if l.contains("install.sh") || l.contains("uninstall.sh") || l.contains("{}") {
                assert!(
                    l.contains("ASSUME_YES=1"),
                    "a privileged script call with no way to answer its prompt: {l}",
                );
            }
        }

        // And the scripts really do prompt, so the requirement above is not
        // guarding something that stopped being true.
        for name in ["install.sh", "uninstall.sh"] {
            let script = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../native/macos/minimal-auth-plugin")
                    .join(name),
            )
            .unwrap_or_default();
            assert!(script.contains("ASSUME_YES"), "{name} no longer honours ASSUME_YES");
        }
    }

    #[test]
    fn the_app_declares_why_it_wants_bluetooth() {
        // Without NSBluetoothAlwaysUsageDescription macOS does not deny the
        // prompt, it kills the process on first CoreBluetooth use. That reads as
        // a crash, not as a permissions problem.
        let plist = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist"),
        )
        .expect("src-tauri/Info.plist should exist");
        assert!(plist.contains("NSBluetoothAlwaysUsageDescription"));
    }

    #[test]
    fn unlock_installer_paths_agree() {
        // install.sh is the source of truth for these; a rename there would
        // otherwise make the panel quietly report "not installed" on a machine
        // that is very much installed.
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../native/macos/minimal-auth-plugin/install.sh"),
        )
        .expect("install.sh should be readable from the crate");
        assert!(script.contains("BUNDLE_NAME=\"ReposeSpike\""), "bundle name changed");
        assert!(script.contains(&format!("SUBRULE=\"{SUBRULE_NAME}\"")), "subrule name changed");
        assert!(script.contains(&format!("DAEMON_LABEL=\"{DAEMON_LABEL}\"")), "daemon label changed");
        assert!(
            BUNDLE_PATH.ends_with("/ReposeSpike.bundle")
                && script.contains("/Library/Security/SecurityAgentPlugins/"),
            "bundle path changed"
        );
    }
}

#[cfg(test)]
mod quoting_tests {
    use super::applescript_quote;

    /// The output of this goes into a command that runs as root. A path that
    /// breaks out of its quotes does not fail, it executes.
    #[test]
    fn a_path_cannot_escape_its_quotes() {
        assert_eq!(applescript_quote("/tmp/plain"), "'/tmp/plain'");
        // Apostrophes in home folder names are ordinary.
        assert_eq!(applescript_quote("/Users/o'brien/x"), "'/Users/o'\\''brien/x'");
        // The shapes someone would try.
        for nasty in ["/tmp/a'; rm -rf /; echo '", "/tmp/$(whoami)", "/tmp/`id`", "/tmp/a b"] {
            let q = applescript_quote(nasty);
            assert!(q.starts_with('\'') && q.ends_with('\''));
            // Every inner apostrophe is closed and reopened, so no odd count
            // can leave the string open.
            assert_eq!(q.matches('\'').count() % 2, 0, "unbalanced quoting for {nasty}");
        }
    }
}
