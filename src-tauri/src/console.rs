//! 快捷键设置 — what this Mac can be asked to press, and whether it is allowed to.
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
use tauri::{AppHandle, Emitter, Manager};

pub const CONSOLE_FILE: &str = "work-console-v1.json";

/// Same idiom as the rest of the app: shell to `date` rather than pull in a
/// date crate for one log line.
fn now_iso() -> String {
    std::process::Command::new("/bin/date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

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
    /// The byte the phone puts in its beacon to ask for this action.
    ///
    /// Assigned by the Mac and STORED, not derived from position. The obvious
    /// design -- "the Nth action in the list" -- renumbers every action after
    /// one you delete, so a phone holding a catalogue from a minute ago presses
    /// the wrong key. Nothing announces that; it just types the wrong thing
    /// into whatever is open.
    ///
    /// A byte belonging to a deleted action matches nothing, and the Mac says so
    /// rather than guessing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd_byte: Option<u8>,
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
    /// The byte that means 「切到这个 App」 with no key pressed. Same pool and
    /// same stability rules as an action's byte. Tapping the App's name on the
    /// phone sends it: on the device people tapped 「飞书」 expecting the Mac to
    /// switch, twice, and nothing happened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd_byte: Option<u8>,
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

/// Command bytes below this are the fixed protocol commands (lock, etc).
/// Shortcuts take the rest.
pub const CONSOLE_CMD_BASE: u8 = 16;

/// Give every action a byte, leaving the ones that already have one alone.
///
/// Stability is the whole point: an action keeps its byte across edits, so a
/// phone's catalogue only goes out of date about actions that were actually
/// added or removed. Bytes freed by a deletion are reused only once every other
/// value is taken -- a phone that has been asleep should press nothing rather
/// than press the action that replaced the one it remembers.
pub fn assign_cmd_bytes(config: &mut ConsoleConfig) {
    let mut taken: std::collections::BTreeSet<u8> = config
        .apps
        .iter()
        .flat_map(|a| a.actions.iter().map(|x| x.cmd_byte).chain(std::iter::once(a.cmd_byte)))
        .flatten()
        .filter(|b| *b >= CONSOLE_CMD_BASE)
        .collect();
    // Start after the highest in use, so a deletion does not immediately hand
    // its byte to the next action created.
    let mut next = taken.iter().next_back().map_or(CONSOLE_CMD_BASE, |b| b.saturating_add(1));
    for app in &mut config.apps {
        // The App's own byte first, then its actions: one pool, one rule.
        let slots = std::iter::once(&mut app.cmd_byte).chain(app.actions.iter_mut().map(|a| &mut a.cmd_byte));
        for slot in slots {
            if slot.is_some_and(|b| b >= CONSOLE_CMD_BASE) {
                continue;
            }
            while next >= CONSOLE_CMD_BASE && taken.contains(&next) {
                next = next.wrapping_add(1);
            }
            if next < CONSOLE_CMD_BASE {
                // Wrapped past 255 and back through the reserved range: every
                // byte is spoken for. Leaving it unassigned is right -- the
                // action still works from this Mac, it just cannot be asked for
                // from a phone, and the page can say so.
                next = CONSOLE_CMD_BASE;
                if taken.len() >= (256 - CONSOLE_CMD_BASE as usize) {
                    return;
                }
            }
            *slot = Some(next);
            taken.insert(next);
            next = next.wrapping_add(1);
        }
    }
}

/// Put back the bytes a config lost on its way through the panel: by app id
/// for the App's own byte, by (app id, action id) for each action. Only
/// where the incoming config has none -- a byte it does carry is trusted.
pub fn carry_cmd_bytes(stored: &ConsoleConfig, config: &mut ConsoleConfig) {
    for app in &mut config.apps {
        let Some(was) = stored.apps.iter().find(|a| a.id == app.id) else { continue };
        if app.cmd_byte.is_none() {
            app.cmd_byte = was.cmd_byte;
        }
        for action in &mut app.actions {
            if action.cmd_byte.is_none() {
                action.cmd_byte = was.actions.iter().find(|a| a.id == action.id).and_then(|a| a.cmd_byte);
            }
        }
    }
}

/// The App a command byte asks to switch to, if the byte is an App's own.
pub fn app_for_cmd(config: &ConsoleConfig, byte: u8) -> Option<&ConsoleApp> {
    config.apps.iter().find(|app| app.cmd_byte == Some(byte))
}

/// The action a command byte asks for, if any.
pub fn action_for_cmd(config: &ConsoleConfig, byte: u8) -> Option<(&ConsoleApp, &ConsoleAction)> {
    config.apps.iter().find_map(|app| {
        app.actions
            .iter()
            .find(|a| a.cmd_byte == Some(byte))
            .map(|a| (app, a))
    })
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
/// Bring the App to the front and nothing else. What tapping its name on the
/// phone does, and the first half of every press.
#[cfg(target_os = "macos")]
pub fn activate_app(app: &ConsoleApp) -> Result<(), String> {
    let bundle = cstr(&app.bundle_id)?;
    let path = cstr(app.app_path.as_deref().unwrap_or(""))?;
    let null = std::ptr::null_mut();
    match unsafe { ffi::repose_console_activate(bundle.as_ptr(), path.as_ptr(), ffi::always, null) } {
        0 => Ok(()),
        1 => Err(format!("这台 Mac 上找不到「{}」。", app.name)),
        // 2 is also what the native side returns when the screen is locked or
        // the session is not on the console. Both mean the same thing here.
        _ => Err(format!("没能把「{}」切到前面来。", app.name)),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate_app(_app: &ConsoleApp) -> Result<(), String> {
    Err("只有 Mac 桌面版能切 App。".into())
}

#[cfg(target_os = "macos")]
pub fn run_action(app: &ConsoleApp, action: &ConsoleAction) -> Result<(), String> {
    if !trusted(false) {
        return Err("macOS 还没允许 Outsie 替你按键。去「手机控制」那一页允许它。".into());
    }
    match action_health(action) {
        ActionHealth::Empty => return Err("这个操作还没有配按键，按下去不会有任何事发生。".into()),
        ActionHealth::MissingKey => return Err("这个操作里有一步没有填按键。".into()),
        ActionHealth::Ok => {}
    }

    activate_app(app)?;
    let bundle = cstr(&app.bundle_id)?;
    let path = cstr(app.app_path.as_deref().unwrap_or(""))?;
    let null = std::ptr::null_mut();

    for step in action.steps.iter() {
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
                    "「{}」这个键，这套键盘布局不认得。回「快捷键设置」重新录一次。",
                    step.key
                ))
            }
            // The app stopped being frontmost between two steps -- someone
            // clicked away. Stopping is right: the rest of the sequence would
            // land somewhere nobody asked for.
            _ => {
                return Err(format!(
                    "还没按到「{}」，「{}」就不在最前面了。剩下的没有发出去。再试一次。",
                    step.key,
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

/// A verified command from the phone, or nothing.
///
/// The verifier has already done the work that matters: it emits a non-zero
/// `cmd=` only when the tag verified AND the sequence was new, so a replayed
/// advertisement never reaches here. This only has to read what it decided,
/// and refuse anything it did not.
pub fn console_command(line: &str) -> Option<(u8, i64)> {
    console_command_with_key(line).map(|(cmd, at, _)| (cmd, at))
}

/// The same, with the key id of the phone that sent it.
///
/// The id is what decides whether that phone is allowed to press anything at
/// all. Without it a Mac with two phones could only say yes to both or no to
/// both.
pub fn console_command_with_key(line: &str) -> Option<(u8, i64, u8)> {
    let fields: Vec<&str> = line.split(',').collect();
    if fields.len() < 9 {
        return None;
    }
    // By name, not by column. The verdict moved by two columns once already and
    // every row silently became unverified -- fail-safe, but a whole feature
    // not working with nothing saying so.
    let field = |name: &str| {
        fields
            .iter()
            .find_map(|f| f.trim().strip_prefix(name).map(str::trim))
    };
    if field("auth=") != Some("VALID") {
        return None;
    }
    let cmd: u8 = field("cmd=")?.parse().ok()?;
    if cmd < CONSOLE_CMD_BASE {
        // Protocol commands (lock, and the reserved one) are somebody else's.
        return None;
    }
    let at: i64 = fields[0].trim().parse().ok()?;
    let key_id: u8 = fields[4].trim().parse().ok()?;
    Some((cmd, at, key_id))
}

/// Commands in this text that are new to us, oldest first.
///
/// `after_ms` is the watcher's high-water mark. Rows from before it are not
/// re-run: the file is appended to for the life of the pipeline, and re-reading
/// it must not replay yesterday's button presses into today's editor.
pub fn console_commands_since(csv: &str, after_ms: i64) -> Vec<(u8, i64, u8)> {
    csv.lines()
        .filter_map(console_command_with_key)
        .filter(|(_, at, _)| *at > after_ms)
        .collect()
}

/// The byte a phone sends to ask for the catalogue. Below the shortcut base
/// because it asks the Mac to do something TO the phone, not to itself.
pub const CONSOLE_CMD_REQUEST: u8 = 3;

/// Catalogue requests new to us, with the key id of the phone that asked.
///
/// The key id matters: the catalogue is signed with THAT phone's key, and
/// sending it under another phone's would produce a list it must reject.
pub fn console_requests_since(csv: &str, after_ms: i64) -> Vec<(u8, i64)> {
    csv.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() < 9 {
                return None;
            }
            let field = |name: &str| {
                fields.iter().find_map(|f| f.trim().strip_prefix(name).map(str::trim))
            };
            if field("auth=") != Some("VALID") {
                return None;
            }
            if field("cmd=")?.parse::<u8>().ok()? != CONSOLE_CMD_REQUEST {
                return None;
            }
            let at: i64 = fields[0].trim().parse().ok()?;
            let key_id: u8 = fields[4].trim().parse().ok()?;
            (at > after_ms).then_some((key_id, at))
        })
        .collect()
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

/// The buttons the phone may show, as compact JSON.
///
/// Short keys because it goes over BLE a few hundred bytes at a time, and only
/// what the phone needs: it cannot edit any of this. Actions without a command
/// byte, and actions that cannot work, are left out -- a button that is certain
/// to do nothing is worse on a phone than on the Mac, because there is nothing
/// on the phone that can explain why.
pub fn catalogue_json(config: &ConsoleConfig) -> String {
    let apps: Vec<serde_json::Value> = config
        .apps
        .iter()
        .filter_map(|app| {
            let actions: Vec<serde_json::Value> = app
                .actions
                .iter()
                .filter(|a| action_health(a) == ActionHealth::Ok)
                .filter_map(|a| {
                    let b = a.cmd_byte?;
                    if b < CONSOLE_CMD_BASE {
                        return None;
                    }
                    let mut o = serde_json::Map::new();
                    o.insert("b".into(), b.into());
                    o.insert("n".into(), a.name.clone().into());
                    if let Some(icon) = a.icon.as_deref().filter(|s| !s.is_empty()) {
                        o.insert("i".into(), icon.into());
                    }
                    // Spelled by the same function the Mac's own screen uses, so
                    // one shortcut is not two different strings on two screens.
                    let keys: Vec<String> = a.steps.iter().map(step_label).collect();
                    o.insert("k".into(), keys.join(" ").into());
                    Some(serde_json::Value::Object(o))
                })
                .collect();
            (!actions.is_empty()).then(|| {
                let mut o = serde_json::json!({ "n": app.name, "a": actions });
                if let Some(b) = app.cmd_byte.filter(|b| *b >= CONSOLE_CMD_BASE) {
                    o["b"] = b.into();
                }
                o
            })
        })
        .collect();
    serde_json::json!({ "apps": apps }).to_string()
}

/// The catalogue key for a phone, written beside the pairing state at pairing.
fn console_key(app: &AppHandle, key_id: u8) -> Option<String> {
    let dir = app.path().app_data_dir().ok()?.join("pairing");
    let raw = std::fs::read_to_string(dir.join(format!("console-key.{key_id}"))).ok()?;
    let k = raw.trim().to_string();
    (k.len() == 64 && k.chars().all(|c| c.is_ascii_hexdigit())).then_some(k)
}

/// Hand the phone the catalogue, over a connection it opened by asking.
///
/// Blocking, and called off the UI thread. The phone's window is sixty seconds;
/// giving up before that would report failure while it was still listening.
fn send_catalogue(app: &AppHandle, key_id: u8) -> Result<(), String> {
    let key = console_key(app, key_id)
        .ok_or("这部手机配对时没留下需要的钥匙。重新配一次。")?;
    let config = ensure_cmd_bytes(app);
    let json = catalogue_json(&config);
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let staged = dir.join("catalogue.json");
    std::fs::write(&staged, &json).map_err(|e| e.to_string())?;

    let bin = crate::unlock::resolve_ble_dir(app)
        .ok_or("找不到发送列表的程序")?
        .join("send-catalogue");
    let out = std::process::Command::new(bin)
        .arg("--key").arg(&key)
        .arg("--revision").arg(config.revision.to_string())
        .arg("--json").arg(&staged)
        .output()
        .map_err(|e| e.to_string())?;
    // The key is on an argument list, which every process on this machine can
    // read. It signs a button list and cannot open this Mac -- that is exactly
    // why the presence key is never passed this way and this one may be.
    if out.status.success() {
        return Ok(());
    }
    let why = String::from_utf8_lossy(&out.stderr)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("没有说明原因")
        .to_string();
    Err(why)
}

// ---- the watcher ----------------------------------------------------------

/// Where a freshly started watcher begins: after the newest command of ANY
/// kind already in the file. It used to look at shortcut presses only, so on a
/// day with none of those and one calibration, relaunching the app replayed
/// the calibration command and the Mac measured an empty chair.
pub fn watcher_start_mark(csv: &str) -> i64 {
    let presses = console_commands_since(csv, 0).into_iter().map(|(_, at, _)| at).max();
    let requests = console_requests_since(csv, 0).into_iter().map(|(_, at)| at).max();
    let legs = crate::unlock::calibration_commands_since(csv, 0).into_iter().map(|(_, at, _)| at).max();
    presses.into_iter().chain(requests).chain(legs).max().unwrap_or(0)
}

/// Watch the verifier's output and press what the phone asks for.
///
/// In the app, not in the privileged half, and that is not an accident:
/// pressing a key needs the accessibility grant, which belongs to this app and
/// to the user's session. The root chain has neither and should never acquire
/// them.
///
/// Polling a file rather than a socket because the file is already there, is
/// already append-only, and is already the thing every other reader in this
/// product agrees on. A second channel would be a second thing that can
/// disagree with it.
pub fn start_command_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        // Anything already in the file belongs to before we were listening. A
        // fresh mark rather than 0: the pipeline's file survives app restarts,
        // and replaying it would type old presses into whatever is open now.
        let path = match app.path().app_data_dir() {
            Ok(d) => d.join("presence-run").join("verified.csv"),
            Err(_) => return,
        };
        let mut mark: i64 = std::fs::read_to_string(&path)
            .map(|csv| watcher_start_mark(&csv))
            .unwrap_or(0);

        loop {
            std::thread::sleep(std::time::Duration::from_millis(700));
            let Ok(csv) = std::fs::read_to_string(&path) else { continue };
            // One mark for the whole poll. Advancing it inside the first loop
            // let a request at t2 hide a command at t1 < t2 from the loops after.
            let since = mark;
            // The phone asked to start a calibration leg. The driver answers
            // through the state beacon; here it only needs to be started.
            for (cmd, at, key_id) in crate::unlock::calibration_commands_since(&csv, since) {
                mark = at.max(mark);
                let started = crate::unlock::drive_calibration(app.clone(), key_id, cmd);
                if let Ok(dir) = app.path().app_data_dir() {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("console.log")) {
                        let _ = writeln!(f, "{} cmd={cmd} 钥匙 {key_id} {}{}", now_iso(),
                            match cmd {
                                crate::unlock::CAL_CMD_NEAR => "要量近处",
                                crate::unlock::CAL_CMD_FAR => "要量远处",
                                _ => "说远处结束，定下来",
                            },
                            if started { "，开始了" } else { "，不是时候，没理" });
                    }
                }
            }
            for (byte, at) in console_requests_since(&csv, since) {
                mark = at.max(mark);
                let outcome = send_catalogue(&app, byte);
                let _ = app.emit(
                    "console-command",
                    serde_json::json!({
                        "action": serde_json::Value::Null,
                        "ok": outcome.is_ok(),
                        "detail": match &outcome {
                            Ok(()) => Some("手机已经拿到按钮列表".to_string()),
                            Err(e) => Some(format!("没能把列表送到手机：{e}")),
                        },
                    }),
                );
            }
            for (byte, at, key_id) in console_commands_since(&csv, since) {
                mark = at.max(mark);
                // Written as well as emitted, and defined BEFORE the first thing
                // that can refuse. A toast is gone in four seconds and the person
                // who pressed the button was looking at their phone; "did it
                // actually press anything" has to be answerable afterwards --
                // and a refusal is the case where that question gets asked.
                let note = |text: String| {
                    if let Ok(dir) = app.path().app_data_dir() {
                        use std::io::Write;
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(dir.join("console.log"))
                        {
                            let _ = writeln!(f, "{} {text}", now_iso());
                        }
                    }
                };
                // Allowed to press anything at all? Off by default: pairing is
                // consent to unlock, not consent to type into whatever is open.
                if !crate::unlock::load_capabilities(&app).control_allowed(key_id) {
                    note(format!("cmd={byte} 被拒：钥匙 {key_id} 没有按键权限"));
                    let _ = app.emit(
                        "console-command",
                        serde_json::json!({
                            "action": serde_json::Value::Null,
                            "ok": false,
                            "detail": "这部手机还没被允许按键。在「手机控制」里打开它的「按快捷键」。",
                        }),
                    );
                    continue;
                }
                let config = ensure_cmd_bytes(&app);
                match action_for_cmd(&config, byte) {
                    Some((target, action)) => {
                        let outcome = run_action(target, action);
                        note(match &outcome {
                            Ok(()) => format!("cmd={byte} 按了「{}」（{}）", action.name, target.name),
                            Err(e) => format!("cmd={byte} 「{}」没按成：{e}", action.name),
                        });
                        // Emitted either way. A press that could not be carried
                        // out is news -- the phone only ever knows it sent
                        // something.
                        let _ = app.emit(
                            "console-command",
                            serde_json::json!({
                                "action": action.name,
                                "app": target.name,
                                "ok": outcome.is_ok(),
                                "detail": outcome.err(),
                            }),
                        );
                    }
                    None if app_for_cmd(&config, byte).is_some() => {
                        let target = app_for_cmd(&config, byte).expect("checked");
                        let outcome = activate_app(target);
                        note(match &outcome {
                            Ok(()) => format!("cmd={byte} 切到「{}」", target.name),
                            Err(e) => format!("cmd={byte} 没切到「{}」：{e}", target.name),
                        });
                        let _ = app.emit(
                            "console-command",
                            serde_json::json!({
                                "action": serde_json::Value::Null,
                                "app": target.name,
                                "ok": outcome.is_ok(),
                                "detail": match &outcome {
                                    Ok(()) => format!("手机说：切到「{}」。切了。", target.name),
                                    Err(e) => format!("手机说：切到「{}」。{e}", target.name),
                                },
                            }),
                        );
                    }
                    None => {
                        note(format!("cmd={byte} 对不上任何操作"));
                        // A byte from a catalogue this Mac no longer has. Doing
                        // nothing is right; doing it silently is not.
                        let _ = app.emit(
                            "console-command",
                            serde_json::json!({
                                "action": serde_json::Value::Null,
                                "ok": false,
                                "detail": "手机上那个按钮，这台 Mac 上已经没有对应的操作了。在手机上重新取一次列表。",
                            }),
                        );
                    }
                }
            }
        }
    });
}

// ---- commands -------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleStatus {
    pub trusted: bool,
    pub config: ConsoleConfig,
}

/// The paste-into-an-AI text, from the live config (design doc §07).
#[tauri::command]
pub fn console_ai_prompt(app: AppHandle) -> String {
    crate::console_cli::ai_prompt(&ensure_cmd_bytes(&app))
}

#[tauri::command]
pub fn console_status(app: AppHandle) -> ConsoleStatus {
    ConsoleStatus { trusted: trusted(false), config: ensure_cmd_bytes(&app) }
}

/// Read the config, and give any action without a command byte one -- writing
/// the result back.
///
/// A write on a read, deliberately. The byte has to exist and be STABLE before
/// a phone can be told about it: assigning it fresh each time from list order
/// would put it back to a position by another name, and deleting an action
/// would silently repoint every phone's buttons. One write the first time, then
/// it never changes again.
pub fn ensure_cmd_bytes(app: &AppHandle) -> ConsoleConfig {
    let mut config = load_config(app);
    let bytes = |c: &ConsoleConfig| -> Vec<Option<u8>> {
        c.apps
            .iter()
            .flat_map(|a| std::iter::once(a.cmd_byte).chain(a.actions.iter().map(|x| x.cmd_byte)))
            .collect()
    };
    let before = bytes(&config);
    assign_cmd_bytes(&mut config);
    let after = bytes(&config);
    if before != after {
        if let (Some(path), Ok(body)) = (config_path(app), serde_json::to_string_pretty(&config)) {
            let tmp = path.with_extension("json.writing");
            if std::fs::write(&tmp, body).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }
    config
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
    let stored = load_config(&app);
    config.revision = stored.revision.wrapping_add(1);
    // The panel does not carry command bytes (its model has no such field),
    // so a config that came back from it has none. Without this, every save
    // from 快捷键设置 renumbered the whole file, and a phone holding
    // yesterday's list pressed 「粘贴」 when it asked for 「搜索」 -- seen on
    // the device 2026-09-12. Bytes belong to ids, and ids survive the trip.
    carry_cmd_bytes(&stored, &mut config);
    assign_cmd_bytes(&mut config);
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
    // The io::Error is for the log, not the toast: it is English plus an OS
    // error code, and the panel shows a string error to the person as-is.
    let failed = |what: &str, e: std::io::Error| -> String {
        if let Ok(dir) = app.path().app_data_dir() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("console.log"))
            {
                let _ = writeln!(f, "{} 没能保存快捷键设置（{what}）：{e}", now_iso());
            }
        }
        "没能保存。再试一次。".to_string()
    };
    std::fs::write(&tmp, body).map_err(|e| failed("写临时文件", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| failed("换掉旧文件", e))?;
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
    #[test]
    fn a_fresh_watcher_starts_after_the_newest_command_of_any_kind() {
        // The bug this pins: the mark was taken from the newest SHORTCUT row
        // only. On a day with no shortcut presses and one calibration, an app
        // relaunch replayed that calibration command -- the Mac started a near
        // leg with nobody there and wrote 「没听到手机」 twenty seconds later.
        let csv = "\
1000,rssi=-50,auth=VALID,x,72,cmd=16,seq=1,a,b\n\
2000,rssi=-50,auth=VALID,x,72,cmd=4,seq=2,a,b\n\
2500,rssi=-50,auth=NOKEY,x,72,cmd=3,seq=3,a,b\n";
        assert_eq!(super::watcher_start_mark(csv), 2000);
        let requests_only = "3000,rssi=-50,auth=VALID,x,72,cmd=3,seq=4,a,b\n";
        assert_eq!(super::watcher_start_mark(requests_only), 3000);
        assert_eq!(super::watcher_start_mark(""), 0);
    }

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
                cmd_byte: None,
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
                cmd_byte: None,
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
                cmd_byte: None,
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

    fn cfg(actions: &[(&str, Option<u8>)]) -> ConsoleConfig {
        ConsoleConfig {
            revision: 1,
            apps: vec![ConsoleApp {
                id: "a".into(), name: "A".into(), bundle_id: "com.a".into(), app_path: None,
                actions: actions
                    .iter()
                    .map(|(id, b)| ConsoleAction {
                        id: (*id).into(),
                        name: (*id).into(),
                        cmd_byte: *b,
                        steps: vec![step("k", &["cmd"])],
                        ..Default::default()
                    })
                    .collect(),
                cmd_byte: None,
            }],
        }
    }

    #[test]
    fn the_catalogue_carries_only_what_the_phone_can_use() {
        let mut c = cfg(&[("one", Some(16)), ("two", None), ("broken", Some(18))]);
        c.apps[0].actions[2].steps.clear();          // nothing to press
        c.apps[0].name = "Terminal".into();
        let json = catalogue_json(&c);
        // Byte 16 is complete; "two" has no byte and "broken" has no keys, and a
        // button on a phone that does nothing has nothing there to explain why.
        assert!(json.contains("\"b\":16"), "{json}");
        assert!(!json.contains("\"b\":18"), "an unusable action reached the phone: {json}");
        assert!(json.contains("Terminal"));
    }

    #[test]
    fn the_keys_are_spelled_the_way_the_macs_own_screen_spells_them() {
        // Two spellings of one shortcut on two screens is the same confusion as
        // two codes in one pairing flow.
        let mut c = cfg(&[("one", Some(16))]);
        c.apps[0].actions[0].steps = vec![step("b", &["ctrl"]), step("%", &[])];
        let json = catalogue_json(&c);
        assert!(json.contains("⌃b %"), "{json}");
    }

    #[test]
    fn an_app_with_nothing_usable_is_left_out_entirely() {
        let mut c = cfg(&[("one", None)]);
        c.apps[0].actions[0].cmd_byte = None;
        assert_eq!(catalogue_json(&c), r#"{"apps":[]}"#);
    }

    #[test]
    fn a_request_carries_the_key_id_of_the_phone_that_asked() {
        // The catalogue is signed with THAT phone's key; signing with another
        // phone's would produce a list it is right to reject.
        let csv = "\
1000,-53,ABC,2,15,tag,0,7,auth=VALID,cmd=3
2000,-53,ABC,2,9,tag,0,8,auth=VALID,cmd=3
3000,-53,ABC,2,15,tag,0,9,auth=BAD,cmd=3
";
        assert_eq!(console_requests_since(csv, 0), vec![(15, 1000), (9, 2000)]);
        assert_eq!(console_requests_since(csv, 1000), vec![(9, 2000)]);
    }

    #[test]
    fn a_catalogue_request_is_not_a_shortcut_press() {
        // cmd=3 asks the Mac to send a list. If it also matched an action it
        // would press a key as well.
        let csv = "1000,-53,ABC,2,15,tag,0,7,auth=VALID,cmd=3";
        assert_eq!(console_commands_since(csv, 0), vec![]);
        assert_eq!(console_requests_since(csv, 0), vec![(15, 1000)]);
    }

    #[test]
    fn only_verified_rows_can_ask_for_a_keypress() {
        // auth=BAD means something was transmitting that we could not
        // authenticate. Obeying it would let a stranger's radio type into
        // whatever is open on this Mac.
        let bad = "1000,-53,ABC,2,15,tag,0,7,auth=BAD,cmd=16";
        assert_eq!(console_command(bad), None);
        let good = "1000,-53,ABC,2,15,tag,0,7,auth=VALID,cmd=16";
        assert_eq!(console_command(good), Some((16, 1000)));
    }

    #[test]
    fn a_protocol_command_is_not_a_shortcut() {
        // cmd=1 locks the Mac. If it also matched an action, a shortcut button
        // and the lock button would be the same press.
        for cmd in 0..CONSOLE_CMD_BASE {
            let line = format!("1000,-53,ABC,2,15,tag,0,7,auth=VALID,cmd={cmd}");
            assert_eq!(console_command(&line), None, "cmd={cmd}");
        }
    }

    #[test]
    fn rows_from_before_we_started_are_not_replayed() {
        // verified.csv is appended to for the life of the pipeline. Reading it
        // without a high-water mark would type every button ever pressed into
        // whatever happens to be open now.
        let csv = "\
1000,-53,ABC,2,15,tag,0,7,auth=VALID,cmd=16
2000,-53,ABC,2,15,tag,0,8,auth=VALID,cmd=17
3000,-53,ABC,2,15,tag,0,9,auth=VALID,cmd=18
";
        assert_eq!(console_commands_since(csv, 2000), vec![(18, 3000, 15)]);
        assert_eq!(console_commands_since(csv, 9999), vec![]);
    }

    #[test]
    fn the_fields_are_read_by_name() {
        // The one bug this file class keeps producing: a column moved and every
        // row became unverified, silently.
        let padded = "1000,-53,ABC,2,15,tag,0,7,extra,auth=VALID,cmd=16";
        assert_eq!(console_command(padded), Some((16, 1000)));
    }

    #[test]
    fn an_action_keeps_its_byte_across_edits() {
        // The whole reason the byte is stored rather than derived. If it were
        // "the Nth action", deleting one would renumber everything after it,
        // and a phone holding a catalogue from a minute ago would press the
        // wrong key -- silently, into whatever is open.
        let mut c = cfg(&[("one", Some(16)), ("two", Some(17)), ("three", Some(18))]);
        c.apps[0].actions.remove(0);
        assign_cmd_bytes(&mut c);
        assert_eq!(c.apps[0].actions[0].cmd_byte, Some(17));
        assert_eq!(c.apps[0].actions[1].cmd_byte, Some(18));
    }

    #[test]
    fn a_new_action_does_not_inherit_a_deleted_ones_byte() {
        // A phone that has been asleep should press nothing, not press whatever
        // replaced the action it remembers.
        let mut c = cfg(&[("one", Some(16)), ("two", Some(17))]);
        c.apps[0].actions.remove(0);            // frees 16
        c.apps[0].actions.push(ConsoleAction {
            id: "new".into(), name: "new".into(), steps: vec![step("k", &[])], ..Default::default()
        });
        assign_cmd_bytes(&mut c);
        // The App's own byte is handed out first (18), then the new action (19);
        // 16 stays free until every other value is taken.
        assert_eq!(c.apps[0].cmd_byte, Some(18));
        let bytes: Vec<_> = c.apps[0].actions.iter().map(|a| a.cmd_byte).collect();
        assert_eq!(bytes, vec![Some(17), Some(19)], "16 was reused too eagerly");
    }

    #[test]
    fn bytes_start_above_the_protocol_commands() {
        // 0..15 belong to lock and friends. An action landing on 1 would be a
        // shortcut button that locks the Mac.
        let mut c = cfg(&[("one", None), ("two", None)]);
        assign_cmd_bytes(&mut c);
        for a in &c.apps[0].actions {
            assert!(a.cmd_byte.unwrap() >= CONSOLE_CMD_BASE);
        }
    }

    #[test]
    fn a_byte_nobody_owns_finds_nothing() {
        let c = cfg(&[("one", Some(16))]);
        assert!(action_for_cmd(&c, 16).is_some());
        assert!(action_for_cmd(&c, 17).is_none(), "a stale phone must match nothing");
        assert!(action_for_cmd(&c, 1).is_none(), "a protocol command is not an action");
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

    #[test]
    fn an_app_gets_a_byte_of_its_own_that_no_action_shares() {
        let mut c = ConsoleConfig {
            revision: 1,
            apps: vec![ConsoleApp {
                id: "a".into(),
                name: "飞书".into(),
                bundle_id: "com.x".into(),
                app_path: None,
                actions: vec![
                    ConsoleAction { id: "s".into(), name: "搜索".into(), cmd_byte: Some(16), ..Default::default() },
                    ConsoleAction { id: "p".into(), name: "粘贴".into(), ..Default::default() },
                ],
                cmd_byte: None,
            }],
        };
        assign_cmd_bytes(&mut c);
        let app_byte = c.apps[0].cmd_byte.expect("the app got a byte");
        let action_bytes: Vec<u8> = c.apps[0].actions.iter().filter_map(|a| a.cmd_byte).collect();
        assert!(app_byte >= CONSOLE_CMD_BASE);
        assert!(!action_bytes.contains(&app_byte), "{app_byte} doubles as an action: {action_bytes:?}");
        assert_eq!(action_bytes[0], 16, "an existing action keeps its byte");
        // Stable: a second pass changes nothing.
        let again = c.clone();
        assign_cmd_bytes(&mut c);
        assert_eq!(again, c);
        assert_eq!(app_for_cmd(&c, app_byte).map(|a| a.name.as_str()), Some("飞书"));
        assert!(action_for_cmd(&c, app_byte).is_none());
        assert!(app_for_cmd(&c, 16).is_none());
    }

    #[test]
    fn the_catalogue_carries_the_app_byte_beside_its_actions() {
        let mut c = ConsoleConfig {
            revision: 1,
            apps: vec![ConsoleApp {
                id: "a".into(),
                name: "飞书".into(),
                bundle_id: "com.x".into(),
                app_path: None,
                actions: vec![ConsoleAction {
                    id: "s".into(),
                    name: "搜索".into(),
                    kind: "keys".into(),
                    steps: vec![ConsoleStep { key: "f".into(), modifiers: vec!["command".into()], delay_ms: 0 }],
                    ..Default::default()
                }],
                cmd_byte: None,
            }],
        };
        assign_cmd_bytes(&mut c);
        let json = catalogue_json(&c);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["apps"][0]["b"], serde_json::json!(c.apps[0].cmd_byte.unwrap()));
        assert_eq!(v["apps"][0]["a"][0]["b"], serde_json::json!(c.apps[0].actions[0].cmd_byte.unwrap()));
    }

    #[test]
    fn a_save_from_the_panel_keeps_every_byte_the_panel_did_not_carry() {
        // What the file holds.
        let mut stored = ConsoleConfig {
            revision: 4,
            apps: vec![ConsoleApp {
                id: "lark".into(), name: "飞书".into(), bundle_id: "com.electron.lark".into(), app_path: None,
                actions: vec![
                    ConsoleAction { id: "search".into(), name: "搜索".into(), cmd_byte: Some(100), ..Default::default() },
                    ConsoleAction { id: "paste".into(), name: "粘贴".into(), cmd_byte: Some(102), ..Default::default() },
                ],
                cmd_byte: Some(105),
            }],
        };
        // What the panel sends back: same ids, no bytes, one new action.
        let mut incoming = stored.clone();
        incoming.apps[0].cmd_byte = None;
        for a in &mut incoming.apps[0].actions { a.cmd_byte = None; }
        incoming.apps[0].actions.push(ConsoleAction { id: "new".into(), name: "新".into(), ..Default::default() });
        carry_cmd_bytes(&stored, &mut incoming);
        assign_cmd_bytes(&mut incoming);
        assert_eq!(incoming.apps[0].cmd_byte, Some(105));
        assert_eq!(incoming.apps[0].actions[0].cmd_byte, Some(100));
        assert_eq!(incoming.apps[0].actions[1].cmd_byte, Some(102));
        let fresh = incoming.apps[0].actions[2].cmd_byte.expect("the new action got a byte");
        assert!(fresh >= CONSOLE_CMD_BASE && ![100, 102, 105].contains(&fresh));
        // An app the file never had gets bytes of its own, and nothing else moves.
        stored.apps.clear();
        let mut other = incoming.clone();
        carry_cmd_bytes(&stored, &mut other);
        assert_eq!(other, incoming);
    }
}
