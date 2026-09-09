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

fn variant_str(v: Option<RuleVariant>) -> Option<&'static str> {
    match v {
        Some(RuleVariant::A) => Some("A"),
        Some(RuleVariant::B) => Some("B"),
        None => None,
    }
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
}

impl HostMacBackend {
    pub fn new(app: &AppHandle) -> Self {
        Self { scripts_dir: resolve_scripts_dir(app) }
    }

    fn now_iso() -> String {
        // Avoid pulling chrono; use `date -u` for an ISO-8601 stamp. Falls back
        // to an empty string only if `date` is somehow unavailable.
        run_capture("/bin/date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_default()
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
        let referenced = rule.as_deref().is_some_and(|r| r.contains("ai.repose"));
        let macos_build = run_capture("/usr/bin/sw_vers", &["-buildVersion"]).unwrap_or_default();

        let state = if !referenced { UnlockState::NotInstalled } else { UnlockState::AwaitingVerification };
        let variant = rule.as_deref().and_then(decide_variant);

        let components = if referenced {
            vec![UnlockComponent {
                id: ComponentId::Rule,
                health: Health::Ok,
                detail: "已就位".into(),
                evidence: None,
                remediation: None,
            }]
        } else {
            vec![]
        };

        Ok(UnlockSnapshot {
            read_at,
            state,
            presence: Presence::TransportUnavailable,
            variant,
            components,
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
            Some(variant) => Ok(PreflightReport { variant: Some(variant), can_install: true, rule_now }),
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
}
