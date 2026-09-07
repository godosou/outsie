use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    ffi::{CString, c_char, c_void},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, RunEvent, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
    menu::MenuBuilder,
    tray::{TrayIconBuilder, TrayIconEvent},
};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn repose_set_strict(enabled: bool);
    fn repose_configure_cover(window: *mut c_void);
    fn repose_idle_seconds() -> f64;
    fn repose_notify(title: *const c_char, body: *const c_char);
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimerStatus {
    running: bool,
    phase: String,
    remaining: f64,
    break_id: Option<String>,
    can_postpone: bool,
    postpone_seconds: u32,
}

impl Default for TimerStatus {
    fn default() -> Self {
        Self {
            running: true,
            phase: "focus".into(),
            remaining: 1200.0,
            break_id: None,
            can_postpone: false,
            postpone_seconds: 60,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    strict_breaks: bool,
    idle_lock_enabled: bool,
    idle_lock_seconds: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            strict_breaks: true,
            idle_lock_enabled: true,
            idle_lock_seconds: 30,
        }
    }
}

#[derive(Clone)]
struct StrictBreak {
    phase: String,
    break_id: String,
    deadline: Instant,
    duration: u32,
    can_postpone: bool,
    postpone_seconds: u32,
    postponing: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BreakSnapshot {
    phase: String,
    remaining: u32,
    duration: u32,
    break_id: String,
    can_postpone: bool,
    postpone_seconds: u32,
    postponing: bool,
}

impl StrictBreak {
    fn snapshot(&self, postponed: bool) -> BreakSnapshot {
        BreakSnapshot {
            phase: self.phase.clone(),
            remaining: self
                .deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64()
                .ceil() as u32,
            duration: self.duration,
            break_id: self.break_id.clone(),
            can_postpone: self.can_postpone && !self.postponing && !postponed,
            postpone_seconds: self.postpone_seconds,
            postponing: self.postponing,
        }
    }
}

struct RuntimeState {
    status: TimerStatus,
    preferences: Preferences,
    strict_break: Option<StrictBreak>,
    completed_break_id: Option<String>,
    pending_postpone_id: Option<String>,
    postponed_break_ids: VecDeque<String>,
    idle_enabled_at: Instant,
    idle_attempted: bool,
    previous_idle: f64,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            status: TimerStatus::default(),
            preferences: Preferences::default(),
            strict_break: None,
            completed_break_id: None,
            pending_postpone_id: None,
            postponed_break_ids: VecDeque::new(),
            idle_enabled_at: Instant::now(),
            idle_attempted: false,
            previous_idle: 0.0,
        }
    }
}

struct SharedState {
    runtime: Mutex<RuntimeState>,
    quitting: AtomicBool,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            runtime: Mutex::new(RuntimeState::default()),
            quitting: AtomicBool::new(false),
        }
    }
}

fn is_break_phase(phase: &str) -> bool {
    matches!(
        phase,
        "short" | "long" | "shortBreak" | "longBreak" | "short-break" | "long-break"
    )
}

fn native_strict(enabled: bool) {
    #[cfg(target_os = "macos")]
    unsafe {
        repose_set_strict(enabled)
    }
}

fn close_break_windows(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("break-") {
            let _ = window.close();
        }
    }
    native_strict(false);
}

fn emit_command(app: &AppHandle, command: &str) {
    let _ = app.emit("repose-command", command);
}

fn create_break_windows(app: &AppHandle) -> tauri::Result<()> {
    let Some(main) = app.get_webview_window("main") else {
        return Ok(());
    };
    let monitors = main.available_monitors()?;
    for (index, monitor) in monitors.into_iter().enumerate() {
        let label = format!("break-{index}");
        if app.get_webview_window(&label).is_some() {
            continue;
        }
        let position: PhysicalPosition<i32> = *monitor.position();
        let size: PhysicalSize<u32> = *monitor.size();
        let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("break.html".into()))
            .title("Repose · 屏幕休息中")
            .position(position.x as f64, position.y as f64)
            .inner_size(size.width as f64, size.height as f64)
            .decorations(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .resizable(false)
            .minimizable(false)
            .maximizable(false)
            .closable(false)
            .focused(index == 0)
            .build()?;
        let _ = window.set_position(position);
        let _ = window.set_size(size);
        #[cfg(target_os = "macos")]
        if let Ok(raw) = window.ns_window() {
            unsafe { repose_configure_cover(raw) }
        }
        let _ = window.show();
        if index == 0 {
            let _ = window.set_focus();
        }
    }
    native_strict(true);
    Ok(())
}

fn start_strict_break(app: &AppHandle, shared: &SharedState, status: &TimerStatus) {
    if status.remaining <= 0.0 || status.break_id.is_none() {
        return;
    }
    let should_create = {
        let mut state = shared.runtime.lock().expect("state poisoned");
        if state.strict_break.is_some()
            || !state.preferences.strict_breaks
            || state.completed_break_id.as_ref() == status.break_id.as_ref()
        {
            false
        } else {
            let break_id = status.break_id.clone().expect("checked above");
            let postponed = state.postponed_break_ids.contains(&break_id);
            let duration = status.remaining.ceil() as u32;
            state.strict_break = Some(StrictBreak {
                phase: status.phase.clone(),
                break_id,
                deadline: Instant::now() + Duration::from_secs_f64(status.remaining),
                duration,
                can_postpone: status.can_postpone && !postponed,
                postpone_seconds: if status.phase.to_ascii_lowercase().contains("long") {
                    300
                } else {
                    60
                },
                postponing: false,
            });
            true
        }
    };
    if should_create && create_break_windows(app).is_err() {
        let mut state = shared.runtime.lock().expect("state poisoned");
        state.strict_break = None;
        drop(state);
        native_strict(false);
    }
}

#[tauri::command]
fn set_status(app: AppHandle, shared: State<'_, Arc<SharedState>>, value: TimerStatus) {
    if value.phase.len() > 40
        || value.remaining < 0.0
        || value.remaining > 604_800.0
        || value
            .break_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 200)
        || !matches!(value.postpone_seconds, 0 | 60 | 300)
    {
        return;
    }

    let mut should_close = false;
    let mut acknowledged_postpone = false;
    {
        let mut state = shared.runtime.lock().expect("state poisoned");
        if let (Some(requested), Some(active)) = (&state.pending_postpone_id, &state.strict_break)
            && value.phase == "focus"
            && value.running
            && value.break_id.as_ref() == Some(requested)
            && requested == &active.break_id
            && !value.can_postpone
        {
            let id = requested.clone();
            if !state.postponed_break_ids.contains(&id) {
                state.postponed_break_ids.push_back(id);
                if state.postponed_break_ids.len() > 128 {
                    state.postponed_break_ids.pop_front();
                }
            }
            state.pending_postpone_id = None;
            state.strict_break = None;
            should_close = true;
            acknowledged_postpone = true;
        }
        if !acknowledged_postpone && !is_break_phase(&value.phase) && state.strict_break.is_some() {
            state.strict_break = None;
            state.pending_postpone_id = None;
            should_close = true;
        }
        if value.phase == "focus" && value.break_id.is_none() {
            state.completed_break_id = None;
        }
        state.status = value.clone();
    }
    if should_close {
        close_break_windows(&app);
    }
    if is_break_phase(&value.phase) {
        start_strict_break(&app, &shared, &value);
    }
}

#[tauri::command]
fn set_preferences(shared: State<'_, Arc<SharedState>>, value: Preferences) {
    if value.idle_lock_seconds != 30 {
        return;
    }
    let mut state = shared.runtime.lock().expect("state poisoned");
    if state.preferences.idle_lock_enabled != value.idle_lock_enabled {
        state.idle_enabled_at = Instant::now();
        state.idle_attempted = false;
        state.previous_idle = 0.0;
    }
    state.preferences = value;
}

#[tauri::command]
fn postpone_break(app: AppHandle, shared: State<'_, Arc<SharedState>>) -> bool {
    let snapshot = {
        let mut state = shared.runtime.lock().expect("state poisoned");
        let Some(active) = state.strict_break.as_ref() else {
            return false;
        };
        if active.postponing
            || !active.can_postpone
            || active.deadline <= Instant::now()
            || state.postponed_break_ids.contains(&active.break_id)
        {
            return false;
        }
        let id = active.break_id.clone();
        state.pending_postpone_id = Some(id);
        state
            .strict_break
            .as_mut()
            .expect("still active")
            .postponing = true;
        let active = state.strict_break.as_ref().expect("still active");
        active.snapshot(state.postponed_break_ids.contains(&active.break_id))
    };
    let _ = app.emit("repose-break-status", snapshot);
    emit_command(&app, "postpone-break");
    true
}

#[derive(Deserialize)]
struct NotificationValue {
    title: String,
    body: String,
}

#[tauri::command]
fn notify_user(value: NotificationValue) {
    if value.title.len() > 200 || value.body.len() > 1_000 {
        return;
    }
    #[cfg(target_os = "macos")]
    if let (Ok(title), Ok(body)) = (CString::new(value.title), CString::new(value.body)) {
        unsafe { repose_notify(title.as_ptr(), body.as_ptr()) }
    }
}

#[tauri::command]
fn open_security_settings() {
    #[cfg(target_os = "macos")]
    let _ = Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status();
}

fn run_break_monitor(app: AppHandle, shared: Arc<SharedState>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(250));
            if shared.quitting.load(Ordering::Relaxed) {
                break;
            }
            let mut finished = false;
            let snapshot = {
                let mut state = shared.runtime.lock().expect("state poisoned");
                let postponed = state
                    .strict_break
                    .as_ref()
                    .is_some_and(|active| state.postponed_break_ids.contains(&active.break_id));
                if let Some(active) = state.strict_break.as_ref() {
                    if active.deadline <= Instant::now() {
                        state.completed_break_id = Some(active.break_id.clone());
                        state.strict_break = None;
                        state.pending_postpone_id = None;
                        finished = true;
                        None
                    } else {
                        Some(active.snapshot(postponed))
                    }
                } else {
                    None
                }
            };
            if let Some(snapshot) = snapshot {
                let _ = app.emit("repose-break-status", snapshot);
            }
            if finished {
                close_break_windows(&app);
                emit_command(&app, "strict-break-finished");
            }
        }
    });
}

fn run_idle_monitor(app: AppHandle, shared: Arc<SharedState>) {
    thread::spawn(move || {
        if std::env::var_os("REPOSE_DISABLE_SESSION_LOCK").is_some() {
            return;
        }
        loop {
            thread::sleep(Duration::from_secs(1));
            if shared.quitting.load(Ordering::Relaxed) {
                break;
            }
            #[cfg(target_os = "macos")]
            let idle = unsafe { repose_idle_seconds() };
            #[cfg(not(target_os = "macos"))]
            let idle = 0.0;
            let should_lock = {
                let mut state = shared.runtime.lock().expect("state poisoned");
                if !state.preferences.idle_lock_enabled {
                    false
                } else {
                    if idle < state.previous_idle {
                        state.idle_attempted = false;
                    }
                    state.previous_idle = idle;
                    let effective = idle.min(state.idle_enabled_at.elapsed().as_secs_f64());
                    if effective >= state.preferences.idle_lock_seconds as f64
                        && !state.idle_attempted
                    {
                        state.idle_attempted = true;
                        true
                    } else {
                        false
                    }
                }
            };
            if should_lock {
                let strict_active = shared
                    .runtime
                    .lock()
                    .expect("state poisoned")
                    .strict_break
                    .is_some();
                if strict_active {
                    native_strict(false);
                }
                let result = Command::new("/usr/bin/osascript")
                .args(["-e", "tell application \"System Events\" to key code 12 using {control down, command down}"])
                .status();
                if !result.is_ok_and(|status| status.success()) {
                    emit_command(&app, "idle-lock-failed");
                }
                if strict_active {
                    thread::sleep(Duration::from_secs(3));
                    if shared
                        .runtime
                        .lock()
                        .expect("state poisoned")
                        .strict_break
                        .is_some()
                    {
                        native_strict(true);
                    }
                }
            }
        }
    });
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("open", "打开 Repose · 歇一会")
        .separator()
        .text("toggle", "暂停／继续提醒")
        .text("short", "现在小休息")
        .text("long", "现在大休息")
        .separator()
        .text("quit", "退出 Repose")
        .build()?;
    // Draw the compact brand flower directly with a transparent background.
    // It stays sage green instead of macOS recoloring it as a black template.
    let mut pixels = vec![0_u8; 18 * 18 * 4];
    for y in 1_i32..17 {
        for x in 1_i32..17 {
            let petal = [(9, 4), (14, 9), (9, 13), (4, 9)]
                .iter()
                .any(|(cx, cy)| (x - cx).pow(2) + (y - cy).pow(2) <= 9);
            let center = (x - 9).pow(2) + (y - 9).pow(2) <= 4;
            let stem = (8..=9).contains(&x) && (9..=16).contains(&y);
            if petal || center || stem {
                let offset = ((y * 18 + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&[0x68, 0x82, 0x58, 0xff]);
            }
        }
    }
    let tray_icon = tauri::image::Image::new_owned(pixels, 18, 18);
    TrayIconBuilder::with_id("repose-tray")
        .menu(&menu)
        .tooltip("Repose · 歇一会")
        .icon(tray_icon)
        .icon_as_template(false)
        .build(app)?;
    Ok(())
}

fn handle_menu(app: &AppHandle, id: &str) {
    let shared = app.state::<Arc<SharedState>>();
    let strict = shared
        .runtime
        .lock()
        .expect("state poisoned")
        .strict_break
        .is_some();
    match id {
        "open" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "toggle" if !strict => emit_command(app, "toggle-pause"),
        "short" if !strict => emit_command(app, "start-short-break"),
        "long" if !strict => emit_command(app, "start-long-break"),
        "quit" if !strict => {
            shared.quitting.store(true, Ordering::Relaxed);
            app.exit(0);
        }
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shared = Arc::new(SharedState::default());
    let app = tauri::Builder::default()
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            set_status,
            set_preferences,
            postpone_break,
            notify_user,
            open_security_settings
        ])
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|app, event| {
            if matches!(event, TrayIconEvent::Click { .. }) {
                handle_menu(app, "open");
            }
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            setup_tray(app)?;
            run_break_monitor(app.handle().clone(), shared.clone());
            run_idle_monitor(app.handle().clone(), shared.clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Repose Lite");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            let shared = app.state::<Arc<SharedState>>();
            let strict = shared
                .runtime
                .lock()
                .expect("state poisoned")
                .strict_break
                .is_some();
            if strict {
                api.prevent_exit();
            } else {
                shared.quitting.store(true, Ordering::Relaxed);
            }
        }
    });
}
