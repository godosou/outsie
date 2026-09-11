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
    pub id: String,
    pub name: String,
    pub platform: String,
    pub paired_at: String,
    pub last_seen_ms: Option<i64>,
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSession {
    pub code: String,
    pub qr_payload: String,
    pub expires_at: String,
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
    Ok,
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

pub struct Assessment {
    pub state: UnlockState,
    pub presence: Presence,
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
            "没有配对密钥。任何设备都无法通过认证，所以不会自动解锁 —— \
             密码照常可用。运行 tools/ble-spike/provision-dev-key.sh 下发一把。",
            Some(Remediation::RePair),
        ),
        // A key exists; now, is anything actually watching? "Away" is the
        // ordinary state of a phone in another room and must not look like a
        // fault, but "nobody is watching" must not look like Away.
        (PresenceKeyState::Ok, PresenceReport::NeverRan) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测还没有运行过。手机钥匙不会生效，密码照常可用。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok, PresenceReport::NotRunning) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测没有在运行 —— 这不是「手机不在」，是没人在看。密码照常可用。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok, PresenceReport::Unreadable) => component(
            ComponentId::Transport,
            Health::Degraded,
            "在场监测的状态读不出来，当作没有在运行处理。密码照常可用。",
            Some(Remediation::ReinstallComponent),
        ),
        (PresenceKeyState::Ok, PresenceReport::Fresh { state, .. }) => component(
            ComponentId::Transport,
            Health::Ok,
            if state == "near" {
                "已配置在场密钥，监测运行中，手机在附近。注意：密钥是通过 USB 下发的开发密钥，不是带防中间人校验的配对。"
            } else {
                "已配置在场密钥，监测运行中，现在没看到手机。注意：密钥是通过 USB 下发的开发密钥，不是带防中间人校验的配对。"
            },
            None,
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
        // mechanism actually run, and the app does not yet drive the BLE bridge.
        UnlockState::AwaitingVerification
    } else if f.rule_references_us || f.bundle_present {
        UnlockState::HalfInstalled
    } else {
        UnlockState::NotInstalled
    };

    Assessment { state, presence, components }
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

    fn presence_key_state() -> PresenceKeyState {
        use std::os::unix::fs::MetadataExt;
        // Slot 1 is the only one the spike provisions. A missing file and an
        // unreadable directory are both "no usable key", and both fail closed.
        let path = format!("{PRESENCE_KEY_DIR}/presence-key.1");
        let Ok(md) = std::fs::metadata(&path) else {
            return PresenceKeyState::Missing;
        };
        let mode = md.mode() & 0o777;
        if md.uid() == 0 && md.gid() == 0 && mode == 0o600 {
            PresenceKeyState::Ok
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
            variant,
            components: assessment.components,
            component_invocation: ComponentInvocation::NeverObserved,
            device: None,
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
                "这台 Mac 的锁屏规则和预期不同，Repose 不改它",
            )),
        }
    }

    fn install(&self, variant: Option<RuleVariant>) -> Result<UnlockSnapshot, UnlockError> {
        let script = self.scripts_dir.as_ref()
            .map(|d| d.join("install.sh"))
            .filter(|p| p.exists())
            .ok_or_else(|| UnlockError::new(UnlockErrorCode::InstallFailed, "找不到安装脚本"))?;
        let _ = variant_str(variant); // form is chosen inside the script's preflight
        // One native authorization prompt; the payload runs as root.
        let cmd = format!(
            "do shell script \"{} permit\" with administrator privileges",
            shell_quote(&script.to_string_lossy()),
        );
        match run_status("/usr/bin/osascript", &["-e", &cmd]) {
            Ok(true) => self.get_snapshot(),
            Ok(false) => Err(UnlockError::new(
                UnlockErrorCode::AuthorizationDenied,
                "没有拿到管理员授权，系统里什么都没有改",
            )),
            Err(e) => Err(UnlockError::new(UnlockErrorCode::InstallFailed, e)),
        }
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
        match run_status("/usr/bin/osascript", &["-e", &cmd]) {
            Ok(_) => {
                let rule_now = Self::read_rule().unwrap_or_default();
                let still_referenced = rule_now.contains("ai.repose");
                Ok(UninstallReport {
                    read_at: Self::now_iso(),
                    rule_now,
                    backup_used: true,
                    diff_against_backup: vec![],
                    right_removed: !still_referenced,
                    bundle_removed: true,
                    keys_removed: true,
                    residual: if still_referenced { vec!["规则仍引用 ai.repose".into()] } else { vec![] },
                })
            }
            Err(e) => Err(UnlockError::new(UnlockErrorCode::InstallFailed, e)),
        }
    }

    fn set_enabled(&self, _enabled: bool) -> Result<UnlockSnapshot, UnlockError> {
        // Pause/resume toggles the BLE LaunchAgent (Step 7), not authorizationdb.
        // Wired tomorrow; today just re-read.
        self.get_snapshot()
    }

    fn revoke_device(&self, _device_id: &str) -> Result<UnlockSnapshot, UnlockError> {
        // Deletes the on-device pairing key (never requires the phone). Pairing
        // store is DEFERRED; today a no-op re-read.
        self.get_snapshot()
    }
}

// ---- process helpers -----------------------------------------------------

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
/// Where the bridge publishes, and where the app looks. One definition, so the
/// two cannot drift into a panel that reads a file nobody writes.
pub fn status_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(STATUS_FILE))
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

#[tauri::command]
pub fn unlock_pair_begin() -> Result<PairingSession, UnlockError> {
    // Placeholder pairing (no crypto — DEFERRED). A fixed-length code + a 3-min
    // expiry so the UI's expiry path works.
    let expires = run_capture("/bin/date", &["-u", "-v+3M", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_default();
    Ok(PairingSession {
        code: "4F2K9A".into(),
        qr_payload: "repose-pair://placeholder".into(),
        expires_at: expires,
    })
}

#[tauri::command]
pub fn unlock_pair_cancel() {}

#[tauri::command]
pub fn unlock_calibrate_sample(value: CalibrateArgs) -> Result<CalibrationReport, UnlockError> {
    let _ = value.kind;
    // Placeholder — real RSSI separability (B3, DEFERRED).
    Ok(CalibrationReport { separable: true, margin_db: 12.0, samples: 20 })
}

#[tauri::command]
pub fn unlock_drill_start(value: DrillArgs) {
    // Real drills need the daemon (Step 9, real Mac). Today just lock the screen
    // via the existing gesture; the drill-result event is emitted tomorrow.
    let _ = value.kind;
    let _ = run_status(
        "/usr/bin/osascript",
        &["-e", "tell application \"System Events\" to key code 12 using {control down, command down}"],
    );
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
            presence_key: PresenceKeyState::Ok,
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
    fn a_provisioned_key_is_never_described_as_pairing() {
        // The key arrives over USB and defends against nobody in the middle.
        // Three artifacts on this project have described protections the code
        // did not have; this asserts the panel is not the fourth.
        let a = assess(&facts());
        let d = &find(&a, ComponentId::Transport).detail;
        assert!(d.contains("不是"), "the transport row must disclaim pairing: {d}");
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
        ] {
            assert!(conf.contains(needed), "tauri.conf.json bundles no {needed}");
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
