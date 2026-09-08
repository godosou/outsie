//! App-scoped keyboard controls. The phone can reference saved actions, never submit keys.
use crate::console_bluetooth::{self, BluetoothTransport};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Step {
    pub key: String,
    pub modifiers: Vec<String>,
    pub delay_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub kind: String,
    pub steps: Vec<Step>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsoleApp {
    pub id: String,
    pub name: String,
    pub bundle_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_path: Option<String>,
    pub actions: Vec<Action>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub revision: u64,
    pub apps: Vec<ConsoleApp>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub config: Config,
    pub enabled: bool,
    pub connected: bool,
    pub running: bool,
    pub active_app_id: Option<String>,
    pub last_error: Option<String>,
    pub accessibility: bool,
    pub blocked: bool,
    pub transport: &'static str,
    pub paired_devices: Vec<console_bluetooth::PairedConsoleDevice>,
    pub bluetooth_ready: bool,
    pub bluetooth_state: &'static str,
}

fn step(key: &str, modifiers: &[&str], delay_ms: u64) -> Step {
    Step {
        key: key.into(),
        modifiers: modifiers.iter().map(|v| (*v).into()).collect(),
        delay_ms,
    }
}
fn action(id: &str, name: &str, icon: &str, steps: Vec<Step>) -> Action {
    Action {
        id: id.into(),
        name: name.into(),
        icon: icon.into(),
        kind: if steps.len() > 1 {
            "sequence"
        } else {
            "hotkey"
        }
        .into(),
        steps,
    }
}
pub fn defaults() -> Config {
    let tmux = |id, name, icon, key| {
        action(
            id,
            name,
            icon,
            vec![step("b", &["ctrl"], 0), step(key, &[], 100)],
        )
    };
    Config {
        revision: 0,
        apps: vec![
            ConsoleApp {
                id: "tmux".into(),
                name: "tmux".into(),
                bundle_id: "com.apple.Terminal".into(),
                app_path: None,
                actions: vec![
                    tmux("split-horizontal", "左右分屏", "◫", "%"),
                    tmux("split-vertical", "上下分屏", "⊟", "\""),
                    tmux("next-pane", "切换分屏", "⇥", "o"),
                    tmux("zoom", "放大 / 还原", "⛶", "z"),
                    tmux("close-pane", "关闭分屏（需确认）", "×", "x"),
                    tmux("new-window", "新建窗口", "+", "c"),
                    action(
                        "workspace",
                        "新建并分屏",
                        "▦",
                        vec![
                            step("b", &["ctrl"], 0),
                            step("c", &[], 100),
                            step("b", &["ctrl"], 300),
                            step("%", &[], 100),
                        ],
                    ),
                ],
            },
            ConsoleApp {
                id: "codex".into(),
                name: "Codex".into(),
                bundle_id: "com.openai.codex".into(),
                app_path: None,
                actions: serde_json::from_str(include_str!("../../src/lib/codexPresets.json"))
                    .expect("valid Codex presets"),
            },
            ConsoleApp {
                id: "feishu".into(),
                name: "飞书".into(),
                bundle_id: "com.electron.lark".into(),
                app_path: None,
                actions: vec![
                    action("search", "搜索", "⌕", vec![step("k", &["meta"], 0)]),
                    action("find", "当前页面查找", "⌘", vec![step("f", &["meta"], 0)]),
                    action("paste", "粘贴", "▣", vec![step("v", &["meta"], 0)]),
                ],
            },
        ],
    }
}
// Upgrade only the untouched original three-button preset. Custom profiles stay intact.
fn upgrade_legacy_codex(mut config: Config) -> Config {
    let legacy = vec![
        action("new-task", "新建任务", "+", vec![step("n", &["meta"], 0)]),
        action("find", "查找", "⌕", vec![step("f", &["meta"], 0)]),
        action("paste", "粘贴", "▣", vec![step("v", &["meta"], 0)]),
    ];
    for app in &mut config.apps {
        if app.id == "codex"
            && app.actions.len() == legacy.len()
            && app.actions.iter().all(|current| {
                legacy
                    .iter()
                    .any(|old| serde_json::to_value(current).ok() == serde_json::to_value(old).ok())
            })
        {
            let presets: Vec<Action> =
                serde_json::from_str(include_str!("../../src/lib/codexPresets.json"))
                    .expect("valid Codex presets");
            for preset in presets {
                if !app.actions.iter().any(|old| old.id == preset.id) {
                    app.actions.push(preset);
                }
            }
        }
    }
    config
}

fn text_valid(value: &str, max: usize) -> bool {
    !value.is_empty() && value.chars().count() <= max && !value.chars().any(char::is_control)
}
fn id_valid(value: &str) -> bool {
    text_valid(value, 64)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
}
pub fn valid_key(key: &str) -> bool {
    (key.len() == 1 && key.as_bytes()[0].is_ascii_graphic())
        || matches!(
            key,
            "Enter"
                | "Tab"
                | "Space"
                | "Escape"
                | "Backspace"
                | "Delete"
                | "ArrowLeft"
                | "ArrowRight"
                | "ArrowUp"
                | "ArrowDown"
                | "Home"
                | "End"
                | "PageUp"
                | "PageDown"
        )
        || key
            .strip_prefix('F')
            .is_some_and(|n| n.parse::<u8>().is_ok_and(|n| (1..=20).contains(&n)))
}
impl Config {
    fn validate(&self) -> Result<(), String> {
        if self.revision > 9_007_199_254_740_990 || self.apps.is_empty() || self.apps.len() > 16 {
            return Err("配置超出容量限制".into());
        }
        let mut ids = HashSet::new();
        for app in &self.apps {
            if !id_valid(&app.id)
                || !ids.insert(&app.id)
                || !text_valid(&app.name, 64)
                || !text_valid(&app.bundle_id, 200)
                || !app.bundle_id.contains('.')
                || !app
                    .bundle_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
                || app.app_path.as_ref().is_some_and(|path| {
                    !text_valid(path, 4096) || !path.starts_with('/') || !path.ends_with(".app")
                })
                || app.actions.len() > 96
            {
                return Err("App 配置无效".into());
            }
            let mut actions = HashSet::new();
            for action in &app.actions {
                if !id_valid(&action.id)
                    || !actions.insert(&action.id)
                    || !text_valid(&action.name, 64)
                    || !text_valid(&action.icon, 16)
                    || !matches!(action.kind.as_str(), "hotkey" | "sequence")
                    || action.steps.is_empty()
                    || action.steps.len() > 20
                    || (action.kind == "hotkey" && action.steps.len() != 1)
                {
                    return Err("操作配置无效".into());
                }
                for s in &action.steps {
                    let unique: HashSet<_> = s.modifiers.iter().collect();
                    if !valid_key(&s.key)
                        || s.delay_ms > 5000
                        || unique.len() != s.modifiers.len()
                        || s.modifiers
                            .iter()
                            .any(|m| !matches!(m.as_str(), "meta" | "ctrl" | "alt" | "shift"))
                    {
                        return Err("按键或等待时间无效".into());
                    }
                }
            }
        }
        Ok(())
    }
}

pub trait Keyboard: Send + Sync {
    fn trusted(&self, prompt: bool) -> bool;
    fn activate(&self, app: &ConsoleApp, valid: &(dyn Fn() -> bool + Sync)) -> Result<(), String>;
    fn send(
        &self,
        app: &ConsoleApp,
        step: &Step,
        valid: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String>;
}
pub struct MacKeyboard;
#[cfg(target_os = "macos")]
struct ExecutionGuard<'a> {
    valid: &'a (dyn Fn() -> bool + Sync),
}
#[cfg(target_os = "macos")]
extern "C" fn execution_valid(context: *mut std::ffi::c_void) -> bool {
    // Native calls this synchronously, including its dispatch_sync main block;
    // the borrowed guard remains alive for the complete FFI call.
    let guard = unsafe { &*context.cast::<ExecutionGuard<'_>>() };
    (guard.valid)()
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn repose_console_trusted(prompt: bool) -> bool;
    fn repose_console_activate(
        bundle: *const std::ffi::c_char,
        path: *const std::ffi::c_char,
        valid: extern "C" fn(*mut std::ffi::c_void) -> bool,
        context: *mut std::ffi::c_void,
    ) -> i32;
    fn repose_console_key(
        bundle: *const std::ffi::c_char,
        path: *const std::ffi::c_char,
        key: *const std::ffi::c_char,
        flags: u32,
        valid: extern "C" fn(*mut std::ffi::c_void) -> bool,
        context: *mut std::ffi::c_void,
    ) -> i32;
}
impl Keyboard for MacKeyboard {
    fn trusted(&self, prompt: bool) -> bool {
        #[cfg(target_os = "macos")]
        {
            unsafe { repose_console_trusted(prompt) }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = prompt;
            false
        }
    }
    fn activate(&self, app: &ConsoleApp, valid: &(dyn Fn() -> bool + Sync)) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let bundle =
                std::ffi::CString::new(app.bundle_id.as_str()).map_err(|_| "App 标识无效")?;
            let path = std::ffi::CString::new(app.app_path.as_deref().unwrap_or(""))
                .map_err(|_| "App 位置无效")?;
            let mut guard = ExecutionGuard { valid };
            match unsafe {
                repose_console_activate(
                    bundle.as_ptr(),
                    path.as_ptr(),
                    execution_valid,
                    (&mut guard as *mut ExecutionGuard).cast(),
                )
            } {
                0 => Ok(()),
                1 => Err("找不到所选 App，请在工作台重新选择".into()),
                _ => Err("无法激活目标 App，控制已停止".into()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, valid);
            Err("仅支持 macOS".into())
        }
    }
    fn send(
        &self,
        app: &ConsoleApp,
        step: &Step,
        valid: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let bundle =
                std::ffi::CString::new(app.bundle_id.as_str()).map_err(|_| "App 标识无效")?;
            let path = std::ffi::CString::new(app.app_path.as_deref().unwrap_or(""))
                .map_err(|_| "App 位置无效")?;
            let key = std::ffi::CString::new(step.key.as_str()).map_err(|_| "按键无效")?;
            let flags = step.modifiers.iter().fold(0, |v, m| {
                v | match m.as_str() {
                    "meta" => 1,
                    "ctrl" => 2,
                    "alt" => 4,
                    "shift" => 8,
                    _ => 0,
                }
            });
            let mut guard = ExecutionGuard { valid };
            match unsafe {
                repose_console_key(
                    bundle.as_ptr(),
                    path.as_ptr(),
                    key.as_ptr(),
                    flags,
                    execution_valid,
                    (&mut guard as *mut ExecutionGuard).cast(),
                )
            } {
                0 => Ok(()),
                1 => Err("请在 Mac 系统设置中允许辅助功能控制".into()),
                2 => Err("前台应用改变或电脑锁定，序列已停止".into()),
                _ => Err("当前键盘布局无法发送此按键".into()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, step, valid);
            Err("仅支持 macOS".into())
        }
    }
}
struct State {
    config: Config,
    enabled: bool,
    heartbeat: Option<Instant>,
    epoch: u64,
    request_ids: HashSet<String>,
    next_run: u64,
    run: Option<(u64, bool, Option<u64>)>,
    cancelled: bool,
    ble_guard: bool,
    active_app_id: Option<String>,
    last_error: Option<String>,
}
pub struct WorkConsole {
    state: Mutex<State>,
    transport: Mutex<Option<BluetoothTransport>>,
    path: PathBuf,
    keyboard: Arc<dyn Keyboard>,
    blocked: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl WorkConsole {
    pub fn new(
        path: PathBuf,
        keyboard: Arc<dyn Keyboard>,
        blocked: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Arc<Self> {
        let loaded = if path.exists() {
            std::fs::read(&path)
                .map_err(|_| ())
                .and_then(|b| {
                    if b.len() > 256 * 1024 {
                        Err(())
                    } else {
                        serde_json::from_slice::<Config>(&b).map_err(|_| ())
                    }
                })
                .and_then(|c| c.validate().map(|_| c).map_err(|_| ()))
        } else {
            Ok(defaults())
        };
        let error = loaded
            .as_ref()
            .err()
            .map(|_| "配置文件损坏，已载入预置；保存后替换原配置".into());
        Arc::new(Self {
            state: Mutex::new(State {
                config: upgrade_legacy_codex(loaded.unwrap_or_else(|_| defaults())),
                enabled: false,
                heartbeat: None,
                epoch: 0,
                request_ids: HashSet::new(),
                next_run: 0,
                run: None,
                cancelled: false,
                ble_guard: false,
                active_app_id: None,
                last_error: error,
            }),
            transport: Mutex::new(None),
            path,
            keyboard,
            blocked,
        })
    }
    pub fn status(&self) -> Status {
        let s = self.state.lock().unwrap();
        Status {
            config: s.config.clone(),
            enabled: s.enabled,
            connected: s.enabled
                && s.heartbeat
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(3)),
            running: s.run.is_some(),
            active_app_id: s.active_app_id.clone(),
            last_error: s.last_error.clone(),
            accessibility: self.keyboard.trusted(false),
            blocked: (self.blocked)(),
            transport: "bluetooth",
            paired_devices: console_bluetooth::paired_devices(),
            bluetooth_ready: console_bluetooth::bluetooth_ready(),
            bluetooth_state: console_bluetooth::bluetooth_state(),
        }
    }
    fn persist(&self, config: &Config) -> Result<(), String> {
        let parent = self.path.parent().ok_or("配置路径无效")?;
        std::fs::create_dir_all(parent).map_err(|_| "无法创建配置目录")?;
        let temp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(config).map_err(|_| "无法编码配置")?;
        std::fs::write(&temp, bytes).map_err(|_| "无法写入配置")?;
        std::fs::rename(temp, &self.path).map_err(|_| "无法保存配置".into())
    }
    fn save_locked(&self, s: &mut State, mut config: Config) -> Result<(), String> {
        config.validate()?;
        if serde_json::to_vec(&config)
            .map_err(|_| "无法编码配置")?
            .len()
            > 192 * 1024
        {
            return Err("配置内容过大，请减少操作或步骤".into());
        }
        if config.revision != s.config.revision {
            return Err("配置已更新，请重新载入后再保存".into());
        }
        config.revision += 1;
        self.persist(&config)?;
        s.config = config;
        s.cancelled = true;
        s.last_error = None;
        Ok(())
    }
    pub fn save(&self, config: Config) -> Result<Status, String> {
        {
            let mut s = self.state.lock().unwrap();
            self.save_locked(&mut s, config)?;
        }
        Ok(self.status())
    }
    pub fn reset(&self, app_id: &str, revision: u64) -> Result<Status, String> {
        {
            let mut s = self.state.lock().unwrap();
            let preset = defaults()
                .apps
                .into_iter()
                .find(|a| a.id == app_id)
                .ok_or("此 App 没有预置")?;
            let mut config = s.config.clone();
            config.revision = revision;
            let app = config
                .apps
                .iter_mut()
                .find(|a| a.id == app_id)
                .ok_or("找不到 App")?;
            app.actions = preset.actions;
            self.save_locked(&mut s, config)?;
        }
        Ok(self.status())
    }
    pub fn cancel(&self) {
        self.state.lock().unwrap().cancelled = true;
    }
    pub fn prompt(&self) -> Status {
        self.keyboard.trusted(true);
        self.status()
    }
    pub fn stop(&self) -> Status {
        // Serialize listener mutation; epoch is checked by each old handler.
        let mut transport = self.transport.lock().unwrap();
        {
            let mut s = self.state.lock().unwrap();
            s.enabled = false;
            s.heartbeat = None;
            s.epoch += 1;
            s.cancelled = true;
        }
        if let Some(t) = transport.take() {
            t.stop();
        }
        self.status()
    }
    pub fn start(self: &Arc<Self>) -> Result<Status, String> {
        let mut transport = self.transport.lock().unwrap();
        let epoch = {
            let mut s = self.state.lock().unwrap();
            s.enabled = false;
            s.heartbeat = None;
            s.epoch += 1;
            s.cancelled = true;
            s.ble_guard = true;
            s.request_ids.clear();
            s.epoch
        };
        if let Some(t) = transport.take() {
            t.stop();
        }
        let weak = Arc::downgrade(self);
        let reset_weak = weak.clone();
        let server = BluetoothTransport::start(
            Arc::new(move |request, generation| {
                weak.upgrade()
                    .ok_or_else(|| "工作台已关闭".into())
                    .and_then(|service| service.ble_request(epoch, request, generation))
            }),
            Arc::new(move || {
                if let Some(service) = reset_weak.upgrade() {
                    service.reset_link(epoch);
                }
            }),
        )?;
        *transport = Some(server);
        self.state.lock().unwrap().enabled = true;
        Ok(self.status())
    }
    fn reset_link(&self, epoch: u64) {
        let mut s = self.state.lock().unwrap();
        if s.epoch != epoch {
            return;
        }
        s.cancelled = true;
        s.heartbeat = None;
        s.request_ids.clear();
    }
    pub fn ble_request(
        self: &Arc<Self>,
        epoch: u64,
        mut value: serde_json::Value,
        generation: u64,
    ) -> Result<serde_json::Value, String> {
        let known = value
            .as_object_mut()
            .and_then(|v| v.remove("knownRevision"))
            .and_then(|v| v.as_u64());
        let status = self.remote_checked(epoch, value, Some(generation))?;
        let same = known == Some(status.config.revision);
        let mut json = serde_json::to_value(status).map_err(|_| "无法生成状态")?;
        if same {
            json.as_object_mut().unwrap().remove("config");
        }
        Ok(json)
    }
    #[cfg(debug_assertions)]
    pub fn start_simulation(&self) -> u64 {
        let mut s = self.state.lock().unwrap();
        s.enabled = true;
        s.ble_guard = true;
        s.epoch += 1;
        s.heartbeat = None;
        s.cancelled = true;
        s.request_ids.clear();
        s.epoch
    }
    #[cfg(test)]
    fn remote(self: &Arc<Self>, epoch: u64, value: serde_json::Value) -> Result<Status, String> {
        self.remote_checked(epoch, value, None)
    }
    fn remote_checked(
        self: &Arc<Self>,
        epoch: u64,
        value: serde_json::Value,
        expected_link: Option<u64>,
    ) -> Result<Status, String> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
        enum Request {
            Status,
            Activate {
                #[serde(rename = "requestId")]
                request_id: String,
                #[serde(rename = "appId")]
                app_id: String,
            },
            Execute {
                #[serde(rename = "requestId")]
                request_id: String,
                #[serde(rename = "appId")]
                app_id: String,
                #[serde(rename = "actionId")]
                action_id: String,
            },
            Cancel {
                #[serde(rename = "requestId")]
                request_id: String,
            },
            Reorder {
                #[serde(rename = "requestId")]
                request_id: String,
                #[serde(rename = "appId")]
                app_id: String,
                revision: u64,
                #[serde(rename = "actionIds")]
                action_ids: Vec<String>,
            },
        }
        let request: Request = serde_json::from_value(value).map_err(|_| "请求格式无效")?;
        {
            let mut s = self.state.lock().unwrap();
            if !s.enabled
                || s.epoch != epoch
                || expected_link.is_some_and(|g| g != console_bluetooth::link_generation())
            {
                return Err("连接已撤销，请重新连接蓝牙".into());
            }
            match &request {
                Request::Status => {
                    s.heartbeat = Some(Instant::now());
                }
                Request::Activate { request_id, .. }
                | Request::Execute { request_id, .. }
                | Request::Cancel { request_id }
                | Request::Reorder { request_id, .. } => {
                    if !text_valid(request_id, 80)
                        || s.request_ids.len() >= 4096
                        || !s.request_ids.insert(request_id.clone())
                    {
                        return Err("请求重复或连接已达上限，请重新连接".into());
                    }
                }
            }
            match request {
                Request::Status => {}
                Request::Cancel { .. } => s.cancelled = true,
                Request::Reorder {
                    app_id,
                    revision,
                    action_ids,
                    ..
                } => {
                    let mut config = s.config.clone();
                    config.revision = revision;
                    let app = config
                        .apps
                        .iter_mut()
                        .find(|a| a.id == app_id)
                        .ok_or("找不到 App")?;
                    if action_ids.len() != app.actions.len()
                        || action_ids.iter().collect::<HashSet<_>>().len() != action_ids.len()
                    {
                        return Err("按钮顺序无效".into());
                    }
                    app.actions = action_ids
                        .iter()
                        .map(|id| {
                            app.actions
                                .iter()
                                .find(|a| &a.id == id)
                                .cloned()
                                .ok_or_else(|| "按钮顺序无效".into())
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    self.save_locked(&mut s, config)?;
                }
                Request::Activate { app_id, .. } => {
                    self.begin_locked(&mut s, &app_id, None, true, expected_link)?
                }
                Request::Execute {
                    app_id, action_id, ..
                } => self.begin_locked(&mut s, &app_id, Some(&action_id), true, expected_link)?,
            }
        }
        Ok(self.status())
    }
    pub fn run(self: &Arc<Self>, app: &str, action: &str) -> Result<Status, String> {
        {
            let mut s = self.state.lock().unwrap();
            self.begin_locked(&mut s, app, Some(action), false, None)?;
        }
        Ok(self.status())
    }
    fn begin_locked(
        self: &Arc<Self>,
        s: &mut State,
        app_id: &str,
        action_id: Option<&str>,
        remote: bool,
        expected_link: Option<u64>,
    ) -> Result<(), String> {
        if s.run.is_some() {
            return Err("已有操作正在执行，请先停止".into());
        }
        if (self.blocked)() {
            return Err("锁屏或休息中，控制已暂停".into());
        }
        if !self.keyboard.trusted(false) {
            return Err("请先允许 Mac 辅助功能控制".into());
        }
        if remote
            && (!s.enabled
                || s.heartbeat
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(3)))
        {
            return Err("手机已断开，请重新连接".into());
        }
        let app = s
            .config
            .apps
            .iter()
            .find(|a| a.id == app_id)
            .ok_or("找不到 App")?
            .clone();
        let steps = match action_id {
            Some(id) => app
                .actions
                .iter()
                .find(|a| a.id == id)
                .ok_or("找不到操作")?
                .steps
                .clone(),
            None => vec![],
        };
        s.next_run += 1;
        let id = s.next_run;
        s.run = Some((
            id,
            remote,
            if s.ble_guard {
                Some(expected_link.unwrap_or_else(console_bluetooth::link_generation))
            } else {
                None
            },
        ));
        s.cancelled = false;
        s.last_error = None;
        let service = self.clone();
        thread::spawn(move || {
            let result = (|| {
                service.check_run(id)?;
                service
                    .keyboard
                    .activate(&app, &|| service.check_run(id).is_ok())?;
                service.check_run(id)?;
                {
                    service.state.lock().unwrap().active_app_id = Some(app.id.clone());
                }
                for step in &steps {
                    let deadline = Instant::now() + Duration::from_millis(step.delay_ms);
                    loop {
                        service.check_run(id)?;
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        thread::sleep((deadline - now).min(Duration::from_millis(20)));
                    }
                    service.check_run(id)?;
                    service
                        .keyboard
                        .send(&app, step, &|| service.check_run(id).is_ok())?;
                }
                Ok::<(), String>(())
            })();
            let mut s = service.state.lock().unwrap();
            if s.run.is_some_and(|r| r.0 == id) {
                s.run = None;
                s.last_error = result.err();
            }
        });
        Ok(())
    }
    fn check_run(&self, id: u64) -> Result<(), String> {
        let s = self.state.lock().unwrap();
        let Some((run, remote, link)) = s.run else {
            return Err("操作已停止".into());
        };
        if run != id
            || (remote && link.is_some_and(|g| g != console_bluetooth::link_generation()))
            || s.cancelled
            || (self.blocked)()
            || (remote
                && (!s.enabled
                    || s.heartbeat
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(3))))
        {
            return Err("操作已停止（取消、断连、锁屏或休息）".into());
        }
        Ok(())
    }
}

#[tauri::command]
pub fn console_status(service: tauri::State<'_, Arc<WorkConsole>>) -> Status {
    service.status()
}
#[tauri::command]
pub fn console_save(
    service: tauri::State<'_, Arc<WorkConsole>>,
    config: Config,
) -> Result<Status, String> {
    service.save(config)
}
#[tauri::command]
pub fn console_reset(
    service: tauri::State<'_, Arc<WorkConsole>>,
    app_id: String,
    revision: u64,
) -> Result<Status, String> {
    service.reset(&app_id, revision)
}
#[tauri::command]
pub fn console_start(service: tauri::State<'_, Arc<WorkConsole>>) -> Result<Status, String> {
    service.inner().start()
}
#[tauri::command]
pub fn console_stop(service: tauri::State<'_, Arc<WorkConsole>>) -> Status {
    service.stop()
}
#[tauri::command]
pub fn console_run(
    service: tauri::State<'_, Arc<WorkConsole>>,
    app_id: String,
    action_id: String,
) -> Result<Status, String> {
    service.inner().run(&app_id, &action_id)
}
#[tauri::command]
pub fn console_cancel(service: tauri::State<'_, Arc<WorkConsole>>) -> Status {
    service.cancel();
    service.status()
}
#[tauri::command]
pub fn console_accessibility(service: tauri::State<'_, Arc<WorkConsole>>) -> Status {
    service.prompt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    #[derive(Default)]
    struct FakeKeys {
        sent: Mutex<Vec<String>>,
        foreground_lost: AtomicBool,
        activated: AtomicUsize,
    }
    impl Keyboard for FakeKeys {
        fn trusted(&self, _: bool) -> bool {
            true
        }
        fn activate(
            &self,
            _: &ConsoleApp,
            valid: &(dyn Fn() -> bool + Sync),
        ) -> Result<(), String> {
            if !valid() {
                return Err("cancelled".into());
            }
            self.activated.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn send(
            &self,
            _: &ConsoleApp,
            step: &Step,
            valid: &(dyn Fn() -> bool + Sync),
        ) -> Result<(), String> {
            if !valid() {
                return Err("cancelled".into());
            }
            if self.foreground_lost.load(Ordering::SeqCst) {
                return Err("foreground changed".into());
            }
            self.sent.lock().unwrap().push(step.key.clone());
            Ok(())
        }
    }
    fn setup() -> (Arc<WorkConsole>, Arc<FakeKeys>, Arc<AtomicBool>) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "repose-console-test-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let keys = Arc::new(FakeKeys::default());
        let blocked = Arc::new(AtomicBool::new(false));
        let b = blocked.clone();
        let service = WorkConsole::new(
            path,
            keys.clone(),
            Arc::new(move || b.load(Ordering::SeqCst)),
        );
        {
            let mut s = service.state.lock().unwrap();
            s.config.apps[0].actions[0].steps = vec![step("b", &["ctrl"], 0), step("%", &[], 200)];
        }
        (service, keys, blocked)
    }
    fn until(predicate: impl Fn() -> bool) {
        let limit = Instant::now() + Duration::from_secs(2);
        while !predicate() {
            assert!(Instant::now() < limit, "worker timed out");
            thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn selected_app_path_persists_and_reset_only_changes_actions() {
        let (service, keys, _) = setup();
        let mut config = defaults();
        config.apps[0].app_path = Some("/Applications/iTerm.app".into());
        config.apps[0].bundle_id = "com.googlecode.iterm2".into();
        config.apps[0].name = "我的终端".into();
        service.save(config).unwrap();
        service.reset("tmux", 1).unwrap();
        let restored = WorkConsole::new(service.path.clone(), keys, Arc::new(|| false));
        let app = &restored.status().config.apps[0];
        assert_eq!(app.app_path.as_deref(), Some("/Applications/iTerm.app"));
        assert_eq!(app.bundle_id, "com.googlecode.iterm2");
        assert_eq!(app.name, "我的终端");
        std::fs::remove_file(&service.path).unwrap();
    }
    #[test]
    fn legacy_defaults_expand_without_overwriting_custom_shortcuts() {
        let mut config = defaults();
        assert_eq!(config.apps[1].actions.len(), 77);
        config.apps[1].actions = vec![
            action("new-task", "新建任务", "+", vec![step("n", &["meta"], 0)]),
            action("find", "查找", "⌕", vec![step("f", &["meta"], 0)]),
            action("paste", "粘贴", "▣", vec![step("v", &["meta"], 0)]),
        ];
        assert_eq!(
            upgrade_legacy_codex(config.clone()).apps[1].actions.len(),
            77
        );
        config.apps[1].actions[0].steps[0].key = "x".into();
        let upgraded = upgrade_legacy_codex(config);
        assert_eq!(upgraded.apps[1].actions.len(), 3);
        assert_eq!(upgraded.apps[1].actions[0].steps[0].key, "x");
    }
    #[test]
    fn app_path_and_expanded_action_limits_are_checked() {
        let mut config = defaults();
        config.apps[0].app_path = Some("relative.app".into());
        assert!(config.validate().is_err());
        config.apps[0].app_path = None;
        config.apps[0].actions = (0..96)
            .map(|i| action(&format!("a{i}"), "Action", "+", vec![step("a", &[], 0)]))
            .collect();
        assert!(config.validate().is_ok());
        config.apps[0]
            .actions
            .push(action("overflow", "Action", "+", vec![step("a", &[], 0)]));
        assert!(config.validate().is_err());
    }
    #[test]
    fn presets_and_serialization_are_valid() {
        let config = defaults();
        config.validate().unwrap();
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["apps"][0]["bundleId"], "com.apple.Terminal");
        let copy: Config = serde_json::from_value(json).unwrap();
        copy.validate().unwrap();
    }
    #[test]
    fn validation_rejects_unknown_keys_duplicate_ids_and_unbounded_steps() {
        let mut c = defaults();
        c.apps[0].actions[0].steps[0].key = "shell command".into();
        assert!(c.validate().is_err());
        c = defaults();
        c.apps[0].actions[0].steps[0].delay_ms = 5001;
        assert!(c.validate().is_err());
        c = defaults();
        c.apps.push(c.apps[0].clone());
        assert!(c.validate().is_err());
        c = defaults();
        c.apps[0].actions[0].steps[0].modifiers = vec!["ctrl".into(), "ctrl".into()];
        assert!(c.validate().is_err());
        c = defaults();
        c.apps[0].bundle_id = "com.apple.Terminal;whoami".into();
        assert!(c.validate().is_err());
    }
    #[test]
    fn save_is_persistent_and_stale_revisions_do_not_overwrite() {
        let (service, keys, _) = setup();
        let mut c = service.status().config;
        c.apps[0].name = "Terminal tools".into();
        assert_eq!(service.save(c.clone()).unwrap().config.revision, 1);
        assert!(service.save(c).is_err());
        let restored = WorkConsole::new(service.path.clone(), keys, Arc::new(|| false));
        assert_eq!(restored.status().config.apps[0].name, "Terminal tools");
        std::fs::remove_file(&service.path).unwrap();
    }
    #[test]
    fn cancelled_or_blocked_sequence_never_sends_remaining_step() {
        for reason in 0..3 {
            let (service, keys, blocked) = setup();
            service.run("tmux", "split-horizontal").unwrap();
            until(|| keys.sent.lock().unwrap().len() == 1);
            match reason {
                0 => service.cancel(),
                1 => blocked.store(true, Ordering::SeqCst),
                _ => keys.foreground_lost.store(true, Ordering::SeqCst),
            }
            until(|| !service.status().running);
            assert_eq!(*keys.sent.lock().unwrap(), ["b"]);
            assert!(service.status().last_error.is_some());
        }
    }
    #[test]
    fn remote_auth_epoch_replay_and_lease_fail_closed() {
        let (service, keys, _) = setup();
        assert!(
            service
                .remote(0, serde_json::json!({"type":"status"}))
                .is_err()
        );
        {
            let mut s = service.state.lock().unwrap();
            s.enabled = true;
            s.epoch = 1;
        }
        assert!(
            service
                .remote(0, serde_json::json!({"type":"status"}))
                .is_err()
        );
        service
            .remote(1, serde_json::json!({"type":"status"}))
            .unwrap();
        let execute = serde_json::json!({"type":"execute","requestId":"one","appId":"tmux","actionId":"split-horizontal"});
        service.remote(1, execute.clone()).unwrap();
        assert!(service.remote(1, execute).is_err());
        until(|| keys.sent.lock().unwrap().len() == 1);
        {
            service.state.lock().unwrap().heartbeat = Some(Instant::now() - Duration::from_secs(4));
        }
        until(|| !service.status().running);
        assert_eq!(*keys.sent.lock().unwrap(), ["b"]);
        service
            .remote(1, serde_json::json!({"type":"status"}))
            .unwrap();
        thread::sleep(Duration::from_millis(230));
        assert_eq!(*keys.sent.lock().unwrap(), ["b"]);
    }
    #[test]
    fn phone_cannot_send_keys_or_replace_actions_and_reorder_is_complete() {
        let (service, _, _) = setup();
        service.state.lock().unwrap().enabled = true;
        assert!(service.remote(0,serde_json::json!({"type":"execute","requestId":"x","appId":"tmux","actionId":"split-horizontal","steps":[{"key":"x"}]})).is_err());
        assert!(service.remote(0,serde_json::json!({"type":"reorder","requestId":"a","appId":"tmux","revision":0,"actionIds":["split-horizontal"]})).is_err());
        let ids: Vec<_> = service.status().config.apps[0]
            .actions
            .iter()
            .rev()
            .map(|a| a.id.clone())
            .collect();
        let updated=service.remote(0,serde_json::json!({"type":"reorder","requestId":"b","appId":"tmux","revision":0,"actionIds":ids})).unwrap();
        assert_eq!(updated.config.apps[0].actions[0].id, "workspace");
        assert_eq!(updated.config.revision, 1);
        std::fs::remove_file(&service.path).unwrap();
    }
    #[test]
    fn concurrent_runs_are_rejected_and_shutdown_cancels() {
        let (service, keys, _) = setup();
        service.run("tmux", "split-horizontal").unwrap();
        assert!(service.run("tmux", "split-horizontal").is_err());
        until(|| keys.sent.lock().unwrap().len() == 1);
        service.stop();
        until(|| !service.status().running);
        assert_eq!(keys.activated.load(Ordering::SeqCst), 1);
        assert_eq!(*keys.sent.lock().unwrap(), ["b"]);
    }
    #[test]
    fn capacity_configuration_survives_save_and_reload() {
        let (service, keys, _) = setup();
        let config = Config {
            revision: 0,
            apps: (0..16)
                .map(|i| ConsoleApp {
                    id: format!("app{i}"),
                    name: "App".into(),
                    bundle_id: "com.apple.Terminal".into(),
                    app_path: None,
                    actions: (0..12)
                        .map(|j| Action {
                            id: format!("a{j}"),
                            name: "Action".into(),
                            icon: "+".into(),
                            kind: "sequence".into(),
                            steps: vec![step("a", &[], 0); 20],
                        })
                        .collect(),
                })
                .collect(),
        };
        service.save(config).unwrap();
        assert!(std::fs::metadata(&service.path).unwrap().len() < 256 * 1024);
        let restored = WorkConsole::new(service.path.clone(), keys, Arc::new(|| false));
        assert_eq!(restored.status().config.apps.len(), 16);
        assert_eq!(restored.status().config.revision, 1);
        assert!(restored.status().last_error.is_none());
        std::fs::remove_file(&service.path).unwrap();
    }
    #[test]
    fn queued_native_dispatch_rechecks_cancellation_before_injection() {
        struct QueuedKeyboard {
            queued: AtomicBool,
            release: AtomicBool,
            sent: AtomicUsize,
        }
        impl Keyboard for QueuedKeyboard {
            fn trusted(&self, _: bool) -> bool {
                true
            }
            fn activate(
                &self,
                _: &ConsoleApp,
                valid: &(dyn Fn() -> bool + Sync),
            ) -> Result<(), String> {
                if valid() {
                    Ok(())
                } else {
                    Err("cancelled".into())
                }
            }
            fn send(
                &self,
                _: &ConsoleApp,
                _: &Step,
                valid: &(dyn Fn() -> bool + Sync),
            ) -> Result<(), String> {
                self.queued.store(true, Ordering::SeqCst);
                until(|| self.release.load(Ordering::SeqCst));
                if !valid() {
                    return Err("cancelled on dispatch".into());
                }
                self.sent.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let keyboard = Arc::new(QueuedKeyboard {
            queued: AtomicBool::new(false),
            release: AtomicBool::new(false),
            sent: AtomicUsize::new(0),
        });
        let service = WorkConsole::new(
            std::env::temp_dir().join("repose-console-dispatch-no-file"),
            keyboard.clone(),
            Arc::new(|| false),
        );
        service.run("tmux", "split-horizontal").unwrap();
        until(|| keyboard.queued.load(Ordering::SeqCst));
        service.cancel();
        keyboard.release.store(true, Ordering::SeqCst);
        until(|| !service.status().running);
        assert_eq!(keyboard.sent.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn delayed_disconnect_from_old_listener_cannot_reset_new_connection() {
        let (service, _, _) = setup();
        let old = service.start_simulation();
        let current = service.start_simulation();
        service
            .remote(current, serde_json::json!({"type":"status"}))
            .unwrap();
        service.reset_link(old);
        assert!(service.status().connected);
        service.reset_link(current);
        assert!(!service.status().connected);
    }
}
