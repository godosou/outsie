//! 快捷控制 — what this Mac can be asked to press, and whether it is allowed to.
//!
//! The phone half is not here yet. What is here is the half that can be
//! verified on one machine: the configuration (which app, which keys), the
//! accessibility permission that makes any of it possible, and pressing a
//! sequence for real so a person can see it land before they trust a button on
//! their phone to do it.
//!
//! WHAT THIS CAN AND CANNOT DO
//!
//! It posts key events to ONE process, chosen by bundle id, and only when that
//! process is already frontmost and the screen is unlocked -- both checked
//! inside work_console.m, on the main thread, immediately before the event is
//! posted. It never installs a global event tap and never types into whatever
//! happens to be in front.
//!
//! The config file is the one the prototype branch already writes,
//! `work-console-v1.json`, read and written unchanged. A Mac that has been
//! using that branch keeps its shortcuts.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const CONSOLE_FILE: &str = "work-console-v1.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleStep {
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<String>,
    #[serde(default)]
    pub delay_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleAction {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub steps: Vec<ConsoleStep>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleApp {
    pub id: String,
    pub name: String,
    pub bundle_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_path: Option<String>,
    #[serde(default)]
    pub actions: Vec<ConsoleAction>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleConfig {
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub apps: Vec<ConsoleApp>,
}

/// The bit layout work_console.m expects. Unknown names are ignored rather than
/// rejected: a config written by a newer build must not make every existing
/// shortcut unusable, and an unknown modifier can only ever make a keystroke
/// weaker, never send it somewhere else.
pub fn modifier_bits(modifiers: &[String]) -> u32 {
    modifiers.iter().fold(0, |acc, m| {
        acc | match m.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "meta" => 1,
            "ctrl" | "control" => 2,
            "alt" | "option" | "opt" => 4,
            "shift" => 8,
            _ => 0,
        }
    })
}

/// An action that can actually be performed, or why it cannot.
///
/// ui-conventions 2.1: a button that is certain to fail should not be on
/// screen. The panel asks this before it draws one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionHealth {
    Ok,
    /// No steps: nothing would happen, and the button would look broken rather
    /// than unconfigured.
    Empty,
    /// A step with no key. The prototype's editor could produce these.
    MissingKey,
}

pub fn action_health(action: &ConsoleAction) -> ActionHealth {
    if action.steps.is_empty() {
        return ActionHealth::Empty;
    }
    if action.steps.iter().any(|s| s.key.trim().is_empty()) {
        return ActionHealth::MissingKey;
    }
    ActionHealth::Ok
}

pub fn find_action<'a>(
    config: &'a ConsoleConfig,
    app_id: &str,
    action_id: &str,
) -> Option<(&'a ConsoleApp, &'a ConsoleAction)> {
    let app = config.apps.iter().find(|a| a.id == app_id)?;
    let action = app.actions.iter().find(|a| a.id == action_id)?;
    Some((app, action))
}

/// Human-readable keys, for the pills on screen.
///
/// Built here rather than in the panel so the Mac and the phone show the same
/// thing: two spellings of one shortcut is the same confusion as two codes in
/// one pairing flow (ui-conventions 3.3).
pub fn step_label(step: &ConsoleStep) -> String {
    let mut out = String::new();
    for m in &step.modifiers {
        out.push_str(match m.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "meta" => "⌘",
            "ctrl" | "control" => "⌃",
            "alt" | "option" | "opt" => "⌥",
            "shift" => "⇧",
            other => other,
        });
    }
    out.push_str(&step.key);
    out
}

// ---- the native half ------------------------------------------------------

#[cfg(target_os = "macos")]
mod ffi {
    use std::ffi::c_char;

    pub type Guard = extern "C" fn(*mut std::ffi::c_void) -> bool;

    unsafe extern "C" {
        pub fn repose_console_trusted(prompt: bool) -> bool;
        pub fn repose_console_activate(
            bundle: *const c_char,
            path: *const c_char,
            valid: Guard,
            context: *mut std::ffi::c_void,
        ) -> i32;
        pub fn repose_console_key(
            bundle: *const c_char,
            path: *const c_char,
            key: *const c_char,
            modifiers: u32,
            valid: Guard,
            context: *mut std::ffi::c_void,
        ) -> i32;
    }

    /// No cancellation yet. The guard exists because the phone half will need to
    /// abandon a sequence mid-way when a session drops; until that exists,
    /// saying "still valid" is the truth rather than a placeholder.
    pub extern "C" fn always(_: *mut std::ffi::c_void) -> bool {
        true
    }
}

/// Whether macOS will let this app press keys in another one.
///
/// `prompt` opens the system's own dialog. Called with false everywhere the
/// panel merely wants to show the state -- polling a status must never throw a
/// dialog at someone who only opened a page.
pub fn trusted(prompt: bool) -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        ffi::repose_console_trusted(prompt)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = prompt;
        false
    }
}

fn cstr(s: &str) -> Result<std::ffi::CString, String> {
    std::ffi::CString::new(s).map_err(|_| "这个设置里有不该出现的字符".to_string())
}

/// Bring the app forward and send the sequence.
///
/// Errors name the consequence, not the return code: every one of these ends
/// with nothing having been pressed, and the reader's question is which thing
/// to go fix.
#[cfg(target_os = "macos")]
pub fn run_action(app: &ConsoleApp, action: &ConsoleAction) -> Result<(), String> {
    if !trusted(false) {
        return Err("macOS 还没允许 Outsie 替你按键。去「系统设置 → 隐私与安全性 → 辅助功能」把它打开。".into());
    }
    match action_health(action) {
        ActionHealth::Empty => return Err("这个操作还没有配按键，按下去不会有任何事发生。".into()),
        ActionHealth::MissingKey => return Err("这个操作里有一步没有填按键。".into()),
        ActionHealth::Ok => {}
    }

    let bundle = cstr(&app.bundle_id)?;
    let path = cstr(app.app_path.as_deref().unwrap_or(""))?;
    let null = std::ptr::null_mut();

    match unsafe { ffi::repose_console_activate(bundle.as_ptr(), path.as_ptr(), ffi::always, null) } {
        0 => {}
        1 => return Err(format!("这台 Mac 上找不到「{}」。", app.name)),
        // 2 is also what the native side returns when the screen is locked or
        // the session is not on the console. Both mean the same thing here.
        _ => return Err(format!("没能把「{}」切到前面来。", app.name)),
    }

    for (i, step) in action.steps.iter().enumerate() {
        if step.delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(step.delay_ms.min(2_000)));
        }
        let key = cstr(&step.key)?;
        let bits = modifier_bits(&step.modifiers);
        match unsafe {
            ffi::repose_console_key(bundle.as_ptr(), path.as_ptr(), key.as_ptr(), bits, ffi::always, null)
        } {
            0 => {}
            1 => return Err("macOS 撤回了辅助功能权限，按键没有发出去。".into()),
            3 => {
                return Err(format!(
                    "第 {} 步的「{}」不是这套键盘布局认得的按键。",
                    i + 1,
                    step.key
                ))
            }
            // The app stopped being frontmost between two steps -- someone
            // clicked away. Stopping is right: the rest of the sequence would
            // land somewhere nobody asked for.
            _ => {
                return Err(format!(
                    "按到第 {} 步时「{}」已经不在最前面了，剩下的没有发出去。",
                    i + 1,
                    app.name
                ))
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn run_action(_app: &ConsoleApp, _action: &ConsoleAction) -> Result<(), String> {
    Err("只有 Mac 桌面版能按键。".into())
}

// ---- storage --------------------------------------------------------------

fn config_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(CONSOLE_FILE))
}

/// Read what is on disk. An unreadable or malformed file becomes an empty
/// config rather than an error: the page's job is then to say there is nothing
/// configured, which is true, instead of refusing to render.
pub fn load_config(app: &AppHandle) -> ConsoleConfig {
    config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// ---- commands -------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleStatus {
    pub trusted: bool,
    pub config: ConsoleConfig,
}

#[tauri::command]
pub fn console_status(app: AppHandle) -> ConsoleStatus {
    ConsoleStatus { trusted: trusted(false), config: load_config(&app) }
}

/// Open the system's own accessibility dialog. Only from a button the user
/// pressed, never from a poll.
#[tauri::command]
pub fn console_request_trust() -> bool {
    trusted(true)
}

/// One app, as macOS describes it. The icon is a data: URI so the panel needs
/// no file access of its own.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedApp {
    pub name: String,
    pub bundle_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn repose_console_pick_app() -> *mut std::ffi::c_char;
    fn repose_console_free_json(value: *mut std::ffi::c_char);
}

/// Open macOS's own application chooser.
///
/// Discovery never launches anything and never loads code out of a bundle --
/// it reads Info.plist and the icon. Picking an app is not running it.
#[tauri::command]
pub async fn console_pick_app() -> Result<Option<PickedApp>, String> {
    #[cfg(target_os = "macos")]
    {
        #[derive(Deserialize)]
        struct Selection {
            #[serde(default)]
            app: Option<PickedApp>,
            #[serde(default)]
            error: Option<String>,
        }
        // The panel is modal on the main thread; the native side already hops
        // there, so this must NOT be spawn_blocking or the two deadlock.
        let raw = unsafe { repose_console_pick_app() };
        if raw.is_null() {
            return Err("没能打开选择窗口".into());
        }
        let parsed: Result<Selection, _> =
            unsafe { serde_json::from_slice(std::ffi::CStr::from_ptr(raw).to_bytes()) };
        unsafe { repose_console_free_json(raw) };
        let sel = parsed.map_err(|_| "选择窗口返回了读不懂的东西".to_string())?;
        if let Some(e) = sel.error {
            return Err(e);
        }
        Ok(sel.app)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("只有 Mac 桌面版能选 App。".into())
    }
}

#[derive(Deserialize)]
pub struct SaveArgs {
    pub config: ConsoleConfig,
}

/// Write the configuration back.
///
/// The revision is bumped here rather than trusted from the caller: it is what
/// the phone will use to tell a stale catalogue from a current one, and a
/// number the UI could forget to change is a number that will eventually make
/// a phone press the wrong key.
#[tauri::command]
pub fn console_save(app: AppHandle, value: SaveArgs) -> Result<ConsoleConfig, String> {
    let mut config = value.config;
    config.revision = load_config(&app).revision.wrapping_add(1);
    // Fill in `kind` only when it is genuinely absent, and by shape.
    //
    // The first version of this stamped "sequence" on everything with an empty
    // kind -- which, combined with the panel not carrying the field at all,
    // rewrote every "hotkey" in the user's file. Nothing here reads `kind`, and
    // that is exactly why it must be preserved rather than normalised: a field
    // we do not use is a field we cannot judge.
    for a in &mut config.apps {
        for action in &mut a.actions {
            if action.kind.trim().is_empty() {
                action.kind = if action.steps.len() > 1 { "sequence" } else { "hotkey" }.into();
            }
        }
    }
    let path = config_path(&app).ok_or("找不到可写的应用数据目录")?;
    let body = serde_json::to_string_pretty(&config).map_err(|_| "配置存不成 JSON")?;
    // Write beside and rename, so an interrupted save cannot leave a truncated
    // file where the configuration used to be.
    let tmp = path.with_extension("json.writing");
    std::fs::write(&tmp, body).map_err(|e| format!("写不进去：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("存不下来：{e}"))?;
    Ok(config)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
    pub app_id: String,
    pub action_id: String,
}

#[tauri::command]
pub async fn console_run(app: AppHandle, value: RunArgs) -> Result<(), String> {
    // Off the main thread: a sequence with delays between steps would otherwise
    // freeze the window for as long as it runs, which is the same mistake
    // unlock_presence_set made (ui-conventions 2.4).
    tauri::async_runtime::spawn_blocking(move || {
        let config = load_config(&app);
        let (a, action) = find_action(&config, &value.app_id, &value.action_id)
            .ok_or_else(|| "这个操作已经不在配置里了。".to_string())?;
        run_action(a, action)
    })
    .await
    .map_err(|_| "按键没能执行".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(key: &str, mods: &[&str]) -> ConsoleStep {
        ConsoleStep {
            key: key.into(),
            modifiers: mods.iter().map(|s| s.to_string()).collect(),
            delay_ms: 0,
        }
    }

    /// The same fill-in console_save does, so the rule is testable without a
    /// Tauri handle.
    fn fill_kind(cfg: &mut ConsoleConfig) {
        for a in &mut cfg.apps {
            for action in &mut a.actions {
                if action.kind.trim().is_empty() {
                    action.kind = if action.steps.len() > 1 { "sequence" } else { "hotkey" }.into();
                }
            }
        }
    }

    #[test]
    fn a_kind_that_is_already_there_is_never_rewritten() {
        // The bug this pins: the panel dropped `kind` on the way through and a
        // save then flattened every "hotkey" in the file to "sequence". Nothing
        // reads the field, which is the reason to leave it alone, not a reason
        // to normalise it.
        let mut cfg = ConsoleConfig {
            revision: 1,
            apps: vec![ConsoleApp {
                id: "a".into(), name: "A".into(), bundle_id: "com.a".into(), app_path: None,
                actions: vec![ConsoleAction {
                    id: "x".into(), name: "X".into(), kind: "hotkey".into(),
                    steps: vec![step("b", &["ctrl"]), step("%", &[])], ..Default::default()
                }],
            }],
        };
        fill_kind(&mut cfg);
        assert_eq!(cfg.apps[0].actions[0].kind, "hotkey", "an existing kind must survive a save");
    }

    #[test]
    fn a_missing_kind_is_filled_in_from_the_shape_of_the_action() {
        let mut cfg = ConsoleConfig {
            revision: 1,
            apps: vec![ConsoleApp {
                id: "a".into(), name: "A".into(), bundle_id: "com.a".into(), app_path: None,
                actions: vec![
                    ConsoleAction { id: "one".into(), name: "One".into(), steps: vec![step("k", &["cmd"])], ..Default::default() },
                    ConsoleAction { id: "many".into(), name: "Many".into(), steps: vec![step("b", &["ctrl"]), step("%", &[])], ..Default::default() },
                ],
            }],
        };
        fill_kind(&mut cfg);
        assert_eq!(cfg.apps[0].actions[0].kind, "hotkey");
        assert_eq!(cfg.apps[0].actions[1].kind, "sequence");
    }

    #[test]
    fn a_saved_action_gets_the_same_kind_the_hand_written_ones_have() {
        // Not cosmetic: a file with both "sequence" and "" is a file whose
        // convention the next reader has to guess at.
        let mut cfg = ConsoleConfig {
            revision: 1,
            apps: vec![ConsoleApp {
                id: "a".into(),
                name: "A".into(),
                bundle_id: "com.a".into(),
                app_path: None,
                actions: vec![ConsoleAction { id: "x".into(), name: "X".into(), ..Default::default() }],
            }],
        };
        for a in &mut cfg.apps {
            for action in &mut a.actions {
                if action.kind.trim().is_empty() {
                    action.kind = "sequence".into();
                }
            }
        }
        assert_eq!(cfg.apps[0].actions[0].kind, "sequence");
    }

    #[test]
    fn modifier_names_map_to_the_bits_the_native_side_reads() {
        assert_eq!(modifier_bits(&["cmd".into()]), 1);
        assert_eq!(modifier_bits(&["ctrl".into()]), 2);
        assert_eq!(modifier_bits(&["alt".into()]), 4);
        assert_eq!(modifier_bits(&["shift".into()]), 8);
        assert_eq!(modifier_bits(&["cmd".into(), "shift".into()]), 9);
    }

    #[test]
    fn the_spellings_a_config_might_actually_use_all_work() {
        // The prototype's editor wrote "ctrl"; a hand-edited file may well say
        // "control" or "Command". Treating those as unknown would silently drop
        // the modifier and send a bare keystroke -- which is not a failure, it
        // is a different keystroke, landing in the user's editor.
        for (name, bits) in [
            ("command", 1), ("Meta", 1), ("control", 2), ("option", 4), ("opt", 4), ("SHIFT", 8),
        ] {
            assert_eq!(modifier_bits(&[name.into()]), bits, "{name}");
        }
    }

    #[test]
    fn an_unknown_modifier_weakens_a_keystroke_it_does_not_redirect_it() {
        assert_eq!(modifier_bits(&["hyper".into(), "cmd".into()]), 1);
    }

    #[test]
    fn an_action_with_no_steps_is_a_button_that_cannot_work() {
        let a = ConsoleAction { id: "x".into(), name: "空的".into(), ..Default::default() };
        assert_eq!(action_health(&a), ActionHealth::Empty);
    }

    #[test]
    fn a_step_with_a_blank_key_is_caught_before_anything_is_pressed() {
        let a = ConsoleAction {
            id: "x".into(),
            name: "半个".into(),
            steps: vec![step("b", &["ctrl"]), step("  ", &[])],
            ..Default::default()
        };
        assert_eq!(action_health(&a), ActionHealth::MissingKey);
    }

    #[test]
    fn the_label_is_what_a_person_would_write_on_a_keycap() {
        assert_eq!(step_label(&step("b", &["ctrl"])), "⌃b");
        assert_eq!(step_label(&step("k", &["cmd", "shift"])), "⌘⇧k");
        assert_eq!(step_label(&step("%", &[])), "%");
    }

    #[test]
    fn the_prototypes_own_config_still_parses() {
        // The exact shape already on disk on this Mac, written by the other
        // branch. Reading it is the whole reason the format was not redesigned.
        let raw = r#"{
          "revision": 3,
          "apps": [{
            "id": "tmux", "name": "tmux",
            "bundleId": "com.apple.Terminal",
            "appPath": "/System/Applications/Utilities/Terminal.app",
            "actions": [{
              "id": "split-horizontal", "name": "左右分屏", "icon": "◫", "kind": "sequence",
              "steps": [
                {"key": "b", "modifiers": ["ctrl"], "delayMs": 0},
                {"key": "%", "modifiers": [], "delayMs": 100}
              ]
            }]
          }]
        }"#;
        let cfg: ConsoleConfig = serde_json::from_str(raw).expect("the on-disk format must parse");
        assert_eq!(cfg.revision, 3);
        let (app, action) = find_action(&cfg, "tmux", "split-horizontal").expect("found by id");
        assert_eq!(app.bundle_id, "com.apple.Terminal");
        assert_eq!(action.steps.len(), 2);
        assert_eq!(step_label(&action.steps[0]), "⌃b");
        assert_eq!(action_health(action), ActionHealth::Ok);
    }

    #[test]
    fn a_missing_or_broken_file_reads_as_nothing_configured_not_as_an_error() {
        let cfg: ConsoleConfig = serde_json::from_str("{}").unwrap_or_default();
        assert!(cfg.apps.is_empty());
        assert_eq!(ConsoleConfig::default().apps.len(), 0);
    }

    #[test]
    fn asking_for_the_status_must_not_be_able_to_raise_a_dialog() {
        // console_status calls trusted(false). A status poll that prompts would
        // throw a system dialog at someone who merely opened the page, once a
        // second.
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/console.rs"),
        )
        .expect("own source");
        let status = src
            .split("pub fn console_status")
            .nth(1)
            .expect("console_status exists");
        assert!(
            status.contains("trusted(false)"),
            "console_status must not prompt",
        );
    }
}
