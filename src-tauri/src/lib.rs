use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    ffi::{CString, c_char, c_void},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum InactivityReason {
    ScreenLock,
    SystemSleep,
    SessionInactive,
}

impl InactivityReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::ScreenLock => "screen-lock",
            Self::SystemSleep => "system-sleep",
            Self::SessionInactive => "session-inactive",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleStarted {
    #[serde(rename = "type")]
    kind: &'static str,
    interval_id: String,
    reason: String,
    started_at: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleInterval {
    #[serde(rename = "type")]
    kind: &'static str,
    interval_id: String,
    elapsed_seconds: f64,
    started_at: u64,
    ended_at: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleSnapshot {
    inactive: bool,
    active_interval: Option<LifecycleStarted>,
    pending_intervals: Vec<LifecycleInterval>,
}

#[derive(Default)]
struct LifecycleGate {
    reasons: HashSet<InactivityReason>,
    active_interval: Option<(LifecycleStarted, f64)>,
    pending_intervals: VecDeque<LifecycleInterval>,
    next_sequence: u64,
}

impl LifecycleGate {
    fn begin(
        &mut self,
        reason: InactivityReason,
        continuous_seconds: f64,
        wall_time_ms: u64,
    ) -> Option<LifecycleStarted> {
        if !self.reasons.insert(reason) || self.active_interval.is_some() {
            return None;
        }
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let started = LifecycleStarted {
            kind: "inactive-start",
            interval_id: format!(
                "{}-{}-{}",
                std::process::id(),
                continuous_seconds.to_bits(),
                self.next_sequence
            ),
            reason: reason.as_str().into(),
            started_at: wall_time_ms,
        };
        self.active_interval = Some((started.clone(), continuous_seconds));
        Some(started)
    }

    fn end(
        &mut self,
        reason: InactivityReason,
        continuous_seconds: f64,
        wall_time_ms: u64,
    ) -> Option<LifecycleInterval> {
        if !self.reasons.remove(&reason) || !self.reasons.is_empty() {
            return None;
        }
        let (started, started_continuous) = self.active_interval.take()?;
        let elapsed_seconds = if continuous_seconds.is_finite() && started_continuous.is_finite() {
            (continuous_seconds - started_continuous).max(0.0)
        } else {
            0.0
        };
        let completed = LifecycleInterval {
            kind: "inactive-end",
            interval_id: started.interval_id,
            elapsed_seconds,
            started_at: started.started_at,
            ended_at: wall_time_ms,
        };
        self.pending_intervals.push_back(completed.clone());
        while self.pending_intervals.len() > 32 {
            self.pending_intervals.pop_front();
        }
        Some(completed)
    }

    fn acknowledge(&mut self, interval_id: &str) -> bool {
        let Some(index) = self
            .pending_intervals
            .iter()
            .position(|interval| interval.interval_id == interval_id)
        else {
            return false;
        };
        self.pending_intervals.remove(index);
        true
    }

    fn snapshot(&self) -> LifecycleSnapshot {
        LifecycleSnapshot {
            inactive: !self.reasons.is_empty(),
            active_interval: self.active_interval.as_ref().map(|(started, _)| started.clone()),
            pending_intervals: self.pending_intervals.iter().cloned().collect(),
        }
    }

    fn is_inactive(&self) -> bool {
        !self.reasons.is_empty()
    }
}
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
    lifecycle: LifecycleGate,
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
            lifecycle: LifecycleGate::default(),
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
fn get_lifecycle_snapshot(shared: State<'_, Arc<SharedState>>) -> LifecycleSnapshot {
    shared
        .runtime
        .lock()
        .expect("state poisoned")
        .lifecycle
        .snapshot()
}

#[tauri::command]
fn acknowledge_lifecycle_interval(
    shared: State<'_, Arc<SharedState>>,
    interval_id: String,
) -> bool {
    if interval_id.is_empty() || interval_id.len() > 200 {
        return false;
    }
    shared
        .runtime
        .lock()
        .expect("state poisoned")
        .lifecycle
        .acknowledge(&interval_id)
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
            get_lifecycle_snapshot,
            acknowledge_lifecycle_interval,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_gate_overlapping_lock_and_sleep_form_one_interval() {
        let mut gate = LifecycleGate::default();
        let started = gate
            .begin(InactivityReason::ScreenLock, 10.0, 1_000)
            .expect("first reason starts an interval");
        assert!(gate
            .begin(InactivityReason::SystemSleep, 12.0, 3_000)
            .is_none());
        assert!(gate
            .end(InactivityReason::SystemSleep, 30.0, 21_000)
            .is_none());
        let completed = gate
            .end(InactivityReason::ScreenLock, 35.0, 26_000)
            .expect("last reason ends the interval");
        assert_eq!(completed.interval_id, started.interval_id);
        assert_eq!(completed.elapsed_seconds, 25.0);
        assert_eq!(completed.started_at, 1_000);
        assert_eq!(completed.ended_at, 26_000);
    }

    #[test]
    fn lifecycle_gate_ignores_duplicate_and_unmatched_notifications() {
        let mut gate = LifecycleGate::default();
        assert!(gate
            .end(InactivityReason::ScreenLock, 1.0, 1_000)
            .is_none());
        assert!(gate
            .begin(InactivityReason::ScreenLock, 2.0, 2_000)
            .is_some());
        assert!(gate
            .begin(InactivityReason::ScreenLock, 3.0, 3_000)
            .is_none());
        assert!(gate
            .end(InactivityReason::ScreenLock, 8.0, 8_000)
            .is_some());
        assert!(gate
            .end(InactivityReason::ScreenLock, 9.0, 9_000)
            .is_none());
    }

    #[test]
    fn lifecycle_gate_replays_pending_interval_until_acknowledged() {
        let mut gate = LifecycleGate::default();
        gate.begin(InactivityReason::SessionInactive, 10.0, 1_000);
        let completed = gate
            .end(InactivityReason::SessionInactive, 20.0, 11_000)
            .expect("interval completes");
        let snapshot = gate.snapshot();
        assert_eq!(snapshot.pending_intervals.len(), 1);
        assert_eq!(snapshot.pending_intervals[0].interval_id, completed.interval_id);
        assert!(!snapshot.inactive);
        assert!(gate.acknowledge(&completed.interval_id));
        assert!(gate.snapshot().pending_intervals.is_empty());
        assert!(!gate.acknowledge(&completed.interval_id));
    }

    #[test]
    fn lifecycle_gate_bounds_unacknowledged_intervals() {
        let mut gate = LifecycleGate::default();
        for index in 0..40 {
            let now = index as f64 * 2.0;
            gate.begin(InactivityReason::ScreenLock, now, index * 2_000);
            gate.end(
                InactivityReason::ScreenLock,
                now + 1.0,
                index * 2_000 + 1_000,
            );
        }
        assert_eq!(gate.snapshot().pending_intervals.len(), 32);
    }
}
