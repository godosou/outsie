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
    /// The key slot it occupies. Today only slot 1 exists; protocol v3 gives
    /// the phone its own id, and then this stops being a constant.
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
    /// Whether this device can unlock right now, and why not when it cannot.
    pub can_unlock: bool,
    pub blocked_reason: Option<String>,
}

/// The shell that deletes one device's key. Pure so the quoting and the fact
/// that it takes the provenance file with it are testable: a key deleted while
/// its provenance stays behind leaves the next key looking SAS-paired when it
/// was pushed over USB.
pub fn revoke_script(key_path: &str) -> String {
    format!(
        "do shell script \"rm -f {key} {key}.provenance\" with administrator privileges",
        key = applescript_quote(key_path),
    )
}

/// Build the row from the facts that exist. There is deliberately no
/// "last seen" field: the bridge publishes its current verdict and keeps no
/// history, so any timestamp here would be invented.
pub fn paired_device(
    key: &PresenceKeyState,
    saved_name: Option<&str>,
    paired_at: Option<String>,
    watching: bool,
) -> Option<PairedDevice> {
    let paired = match key {
        // No key, or a key the verifier would refuse: nothing can unlock, so
        // the list is empty rather than showing a device that cannot work.
        PresenceKeyState::Missing | PresenceKeyState::BadPermissions { .. } => return None,
        PresenceKeyState::Ok { paired } => *paired,
    };
    let name = match (paired, saved_name.map(str::trim).filter(|n| !n.is_empty())) {
        (true, Some(n)) => n.to_string(),
        // Paired, but the cosmetic name was never written or was lost. The row
        // still belongs here; only its label is unknown.
        (true, None) => "已配对的手机".to_string(),
        (false, _) => "USB 下发的开发密钥".to_string(),
    };
    Some(PairedDevice {
        id: "1".to_string(),
        name,
        platform: if paired { "Android" } else { "开发用" }.to_string(),
        paired_at: paired_at.unwrap_or_default(),
        paired,
        can_unlock: watching,
        blocked_reason: (!watching).then(|| "上面的开关关着，现在谁都解不了锁".to_string()),
    })
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
    pub device: Option<PairedDevice>,
    pub stats: UnlockStats,
    pub last_failure: Option<serde_json::Value>,
    pub macos_build: String,
    pub component_version: String,
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationReport {
    pub separable: bool,
    pub margin_db: f64,
    pub samples: u32,
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
pub struct CalibrateArgs {
    pub kind: String,
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
            "组件已安装，但锁屏规则没有引用它 —— 解锁不会发生，密码照常可用。",
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
            "还没有和手机配对。任何设备都无法通过认证，所以不会自动解锁 —— \
             密码照常可用。点上面的「配对手机」，一分钟就能配好。",
            Some(Remediation::RePair),
        ),
        // A key exists; now, is anything actually watching? "Away" is the
        // ordinary state of a phone in another room and must not look like a
        // fault, but "nobody is watching" must not look like Away.
        (PresenceKeyState::Ok { .. }, PresenceReport::NeverRan) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测还没有运行过。手机钥匙不会生效，密码照常可用。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok { .. }, PresenceReport::NotRunning) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测没有在运行 —— 这不是「手机不在」，是没人在看。密码照常可用。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok { .. }, PresenceReport::NoAuthorization) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测没有拿到管理员授权，所以只有扫描在跑，没有任何东西在验证 —— \
             手机钥匙不会生效，密码照常可用。把开关关掉再打开，这次在密码框里完成授权。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok { .. }, PresenceReport::Unreadable) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测的状态读不出来，当作没有在运行处理。密码照常可用。",
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
            presence_key: Self::presence_key_state(),
            presence: Self::presence_report(app),
        }
    }

    /// The nickname the phone reported during pairing, saved next to the
    /// pairing state. Cosmetic, and absent on a Mac paired before it was
    /// recorded -- both are fine; the card falls back to a generic label.
    fn saved_peer_name(app: &AppHandle) -> Option<String> {
        let dir = app.path().app_data_dir().ok()?.join("pairing");
        std::fs::read_to_string(dir.join("peer-name.saved")).ok()
    }

    /// When the key file was written, which is when pairing finished.
    fn key_written_at() -> Option<String> {
        let md = std::fs::metadata(format!("{PRESENCE_KEY_DIR}/presence-key.1")).ok()?;
        let secs = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        // Same idiom as now_iso: shell to `date` rather than pull a date crate
        // in for two call sites.
        run_capture("/bin/date", &["-u", "-r", &secs.to_string(), "+%Y-%m-%dT%H:%M:%SZ"])
    }

    fn presence_key_state() -> PresenceKeyState {
        use std::os::unix::fs::MetadataExt;
        // Slot 1 is the only one the spike provisions. A missing file and an
        // unreadable directory are both "no usable key", and both fail closed.
        let path = format!("{PRESENCE_KEY_DIR}/presence-key.1");
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
                .map(|s| s.trim() == "repose-pair-v2")
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
            device: paired_device(
                &Self::presence_key_state(),
                Self::saved_peer_name(&self.app).as_deref(),
                Self::key_written_at(),
                assessment.presence_running,
            ),
            stats: UnlockStats { unlocks_today: 0, last_unlock_at: None },
            last_failure: None,
            macos_build,
            component_version: env!("CARGO_PKG_VERSION").to_string(),
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
        // Slot 1 is the only slot that exists. Accepting any id and deleting
        // slot 1 would delete the wrong key the day a second one exists.
        if device_id != "1" {
            return Err(UnlockError::new(
                UnlockErrorCode::Unsupported,
                format!("这台 Mac 上没有编号 {device_id} 的钥匙"),
            ));
        }
        let key = format!("{PRESENCE_KEY_DIR}/presence-key.1");
        run_privileged(&revoke_script(&key))?;

        // Read back before reporting. The password prompt was the user's, and
        // what it bought them has to be checked, not assumed.
        if std::fs::metadata(&key).is_ok() {
            return Err(UnlockError::new(
                UnlockErrorCode::InstallFailed,
                format!("{key} 还在。这部手机仍然可以解锁这台 Mac。"),
            ));
        }
        // The nickname is only meaningful next to the key it named.
        if let Ok(dir) = self.app.path().app_data_dir() {
            let _ = std::fs::remove_file(dir.join("pairing").join("peer-name.saved"));
        }
        self.get_snapshot()
    }
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
    // Install and pairing reach this from a synchronous Tauri command, which
    // runs on the main thread. The presence pipeline reaches it from a thread
    // of its own, deliberately -- `do shell script` does not return until the
    // command does, and that one runs for as long as monitoring does, so
    // blocking the main thread with it would freeze the window for the whole
    // session rather than for the length of a dialog.
    //
    // One instance, one thread, never shared: that is the shape that is safe.
    // It has been exercised on this Mac; if it ever misbehaves, the alternative
    // is Authorization Services directly, which is more code and the same
    // dialog.
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
fn resolve_ble_dir(app: &AppHandle) -> Option<PathBuf> {
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

    let child = Command::new("/bin/bash")
        .arg(dir.join("presence-pipeline.sh"))
        .arg("0") // run until stopped
        .env("REPOSE_STATUS_FILE", &status)
        .env("REPOSE_PIPELINE_DIR", &work)
        // We raise the authorization ourselves, below.
        .env("REPOSE_SKIP_PRIVILEGED", "1")
        // Local target: the plugin is on this machine, so the permit is a
        // local root-owned file and the whole privileged half is one
        // prompt. REPOSE_SSH being absent is what selects that.
        .env_remove("REPOSE_SSH")
        .spawn()
        .map_err(|e| UnlockError::new(UnlockErrorCode::Unsupported, e.to_string()))?;

    // Wait for the pipeline to lay out its files before root goes looking for
    // them. The run flag is the last thing it creates before it would have
    // asked for root itself.
    let runflag = work.join("running");
    for _ in 0..40 {
        if runflag.exists() { break }
        std::thread::sleep(std::time::Duration::from_millis(100));
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
        "do shell script \"REPOSE_MODE=local REPOSE_BIN={bin} REPOSE_KEY_DIR={keys} \
         REPOSE_RAW={raw} REPOSE_VERIFIED={verified} REPOSE_RUNFLAG={flag} \
         REPOSE_PERMIT_DIR={permit} REPOSE_STATUS_FILE={status} REPOSE_LOG_DIR={work} \
         {priv_sh}\" with administrator privileges",
        bin = applescript_quote(&dir.to_string_lossy()),
        keys = applescript_quote(PRESENCE_KEY_DIR),
        raw = applescript_quote(&work.join("raw.csv").to_string_lossy()),
        verified = applescript_quote(&work.join("verified.csv").to_string_lossy()),
        flag = applescript_quote(&runflag.to_string_lossy()),
        permit = applescript_quote("/var/run/repose-spike"),
        status = applescript_quote(&status.to_string_lossy()),
        work = applescript_quote(&work.to_string_lossy()),
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

#[tauri::command]
pub fn unlock_install(app: AppHandle, value: InstallArgs) -> Result<UnlockSnapshot, UnlockError> {
    let variant = match value.variant.as_deref() {
        Some("A") => Some(RuleVariant::A),
        Some("B") => Some(RuleVariant::B),
        _ => None,
    };
    HostMacBackend::new(&app).install(variant)
}

#[tauri::command]
pub fn unlock_repair(app: AppHandle, value: RepairArgs) -> Result<UnlockSnapshot, UnlockError> {
    HostMacBackend::new(&app).repair(&value.target)
}

#[tauri::command]
pub fn unlock_uninstall(app: AppHandle) -> Result<UninstallReport, UnlockError> {
    HostMacBackend::new(&app).uninstall()
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
pub fn unlock_revoke_device(app: AppHandle, value: DeviceArgs) -> Result<UnlockSnapshot, UnlockError> {
    HostMacBackend::new(&app).revoke_device(&value.device_id)
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
pub fn unlock_pair_confirm(app: AppHandle) -> Result<PairingSession, UnlockError> {
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
    let key = key.trim().to_string();

    if code != Some(0) || key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
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
    let script = format!(
        "do shell script \"mkdir -p {KEY_DIR} && chown root:wheel {KEY_DIR} && chmod 755 {KEY_DIR} \
         && install -m 600 -o root -g wheel '{staged_path}' {KEY_DIR}/presence-key.1 \
         && echo repose-pair-v2 > {KEY_DIR}/presence-key.1.provenance \
         && chmod 644 {KEY_DIR}/presence-key.1.provenance\" with administrator privileges",
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
    let key_path = format!("{PRESENCE_KEY_DIR}/presence-key.1");
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
    if let (Some(n), Ok(dir)) = (&peer_name, pairing_dir(&app)) {
        let _ = std::fs::write(dir.join("peer-name.saved"), n);
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

#[tauri::command]
pub fn unlock_calibrate_sample(value: CalibrateArgs) -> Result<CalibrationReport, UnlockError> {
    let _ = value.kind;
    // Placeholder — real RSSI separability (B3, DEFERRED).
    Ok(CalibrationReport { separable: true, margin_db: 12.0, samples: 20 })
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
            device: None,
            stats: UnlockStats { unlocks_today: 0, last_unlock_at: None },
            last_failure: None,
            macos_build: "b".into(),
            component_version: "0".into(),
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
        assert!(find(&a, ComponentId::Component).detail.contains("密码照常可用"));
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
        assert!(t.detail.contains("密码照常可用"));
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
    fn no_usable_key_means_an_empty_list() {
        // A device row is a promise that something can unlock this Mac. With no
        // key, or a key the verifier refuses, nothing can -- and a row saying
        // otherwise is the bug this whole file is written against.
        assert!(paired_device(&PresenceKeyState::Missing, Some("realme GT5 Pro"), None, true).is_none());
        assert!(paired_device(
            &PresenceKeyState::BadPermissions { detail: String::new() },
            Some("realme GT5 Pro"),
            None,
            true
        )
        .is_none());
    }

    #[test]
    fn a_dev_key_is_listed_but_not_called_a_paired_phone() {
        // It really can unlock this Mac, so hiding it would be a lie by
        // omission; calling it a paired phone would be the opposite lie.
        let d = paired_device(&PresenceKeyState::Ok { paired: false }, Some("realme GT5 Pro"), None, true)
            .expect("a usable key is a device that can unlock");
        assert!(!d.paired);
        assert!(!d.name.contains("realme"), "a name from a previous pairing must not label a dev key: {}", d.name);
    }

    #[test]
    fn a_paired_phone_wears_the_name_it_reported() {
        let d = paired_device(&PresenceKeyState::Ok { paired: true }, Some("  realme GT5 Pro "), None, true).unwrap();
        assert_eq!(d.name, "realme GT5 Pro");
        assert!(d.paired);
        assert!(d.can_unlock);
        assert!(d.blocked_reason.is_none());
    }

    #[test]
    fn a_paired_phone_without_a_saved_name_still_gets_a_row() {
        // The nickname is cosmetic and its absence must not delete the device.
        for name in [None, Some(""), Some("   ")] {
            let d = paired_device(&PresenceKeyState::Ok { paired: true }, name, None, true).unwrap();
            assert_eq!(d.name, "已配对的手机");
        }
    }

    #[test]
    fn a_phone_that_cannot_unlock_says_why() {
        // ui-conventions 1.1: the row shows what is true now, not what pairing
        // once achieved. With the monitor stopped, this phone opens nothing.
        let d = paired_device(&PresenceKeyState::Ok { paired: true }, Some("realme"), None, false).unwrap();
        assert!(!d.can_unlock);
        assert!(d.blocked_reason.is_some(), "a disabled row must carry its reason");
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
            assert!(t.detail.contains("密码照常可用") || t.detail.contains("没有在运行"), "{report:?}: {}", t.detail);
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
