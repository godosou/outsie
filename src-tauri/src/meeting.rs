//! Is a video meeting going on? Judged by which processes hold audio.
//!
//! Camera state and microphone state are the wrong signals: the user rarely
//! turns the camera on and often sits muted. What a meeting app cannot stop
//! doing is playing the other side, so a meeting is "a known meeting app's
//! process has an input or output audio stream open". Verified against a real
//! Feishu call on 2026-09-12 (docs/plans/2026-09-12-meeting-detection-research.md):
//! the signal holds while muted and clears the moment the call ends. The
//! renderer owns what a meeting means for the timer; this module only reports.

use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

use crate::SharedState;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AudioProcess {
    pub bundle: String,
    pub name: String,
    #[serde(deserialize_with = "truthy")]
    pub input: bool,
    #[serde(deserialize_with = "truthy")]
    pub output: bool,
}

/// CoreAudio hands these over as UInt32; accept a JSON bool or a number, so a
/// boxing slip on the native side can never again read as "no meeting".
fn truthy<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Flag {
        Bool(bool),
        Number(f64),
    }
    Ok(match Flag::deserialize(deserializer)? {
        Flag::Bool(value) => value,
        Flag::Number(value) => value != 0.0,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RunningApp {
    pub bundle: String,
    pub name: String,
}

/// What the desktop reports once a second: who holds audio, who is running.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Activity {
    pub audio: Vec<AudioProcess>,
    pub running: Vec<RunningApp>,
}

/// Bundle identifiers of meeting apps, matched by prefix so helper processes
/// count (Feishu runs its calls in `com.electron.lark.iron`).
const MEETING_BUNDLE_PREFIXES: &[&str] = &[
    "us.zoom.",
    "com.microsoft.teams",
    "com.electron.lark",
    "com.larksuite",
    "com.tencent.meeting",
    "com.apple.FaceTime",
];

/// Helper processes that only exist inside a call. Zoom spawns CptHost per meeting.
const MEETING_HELPER_NAMES: &[&str] = &["CptHost"];

pub fn is_meeting_bundle(bundle: &str) -> bool {
    MEETING_BUNDLE_PREFIXES.iter().any(|prefix| bundle.starts_with(prefix))
}

pub fn meeting_active(activity: &Activity) -> bool {
    let audio = activity
        .audio
        .iter()
        .any(|process| (process.input || process.output) && is_meeting_bundle(&process.bundle));
    let helper = activity
        .running
        .iter()
        .any(|app| MEETING_HELPER_NAMES.contains(&app.name.as_str()));
    audio || helper
}

/// Consecutive one-second samples that must agree before the answer flips.
/// Apps open and close streams for a moment while joining or leaving.
pub const DEBOUNCE_SAMPLES: u8 = 5;

#[derive(Debug, Default)]
pub struct Debounce {
    active: bool,
    streak: u8,
}

impl Debounce {
    pub fn active(&self) -> bool {
        self.active
    }

    /// Feed one raw sample; returns the new answer only when it flips.
    pub fn observe(&mut self, raw: bool) -> Option<bool> {
        if raw == self.active {
            self.streak = 0;
            return None;
        }
        self.streak += 1;
        if self.streak < DEBOUNCE_SAMPLES {
            return None;
        }
        self.active = raw;
        self.streak = 0;
        Some(raw)
    }
}

#[derive(Clone, Serialize)]
struct MeetingEvent {
    active: bool,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn repose_activity_json() -> *mut std::ffi::c_char;
    fn repose_free_json(value: *mut std::ffi::c_char);
}

#[cfg(target_os = "macos")]
fn read_activity() -> Option<Activity> {
    let raw = unsafe { repose_activity_json() };
    if raw.is_null() {
        return None;
    }
    let parsed = unsafe { serde_json::from_slice(std::ffi::CStr::from_ptr(raw).to_bytes()) };
    unsafe { repose_free_json(raw) };
    parsed.ok()
}

#[cfg(not(target_os = "macos"))]
fn read_activity() -> Option<Activity> {
    None
}

/// The renderer asks once on launch; afterwards it listens for `repose-meeting`.
#[tauri::command]
pub fn get_meeting_state() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn run_meeting_monitor(app: AppHandle, shared: Arc<SharedState>) {
    thread::spawn(move || {
        if std::env::var_os("REPOSE_DISABLE_MEETING_DETECTION").is_some() {
            return;
        }
        let mut debounce = Debounce::default();
        loop {
            thread::sleep(Duration::from_secs(1));
            if shared.quitting.load(Ordering::Relaxed) {
                break;
            }
            let raw = read_activity().is_some_and(|activity| meeting_active(&activity));
            if let Some(active) = debounce.observe(raw) {
                ACTIVE.store(active, Ordering::Relaxed);
                let _ = app.emit("repose-meeting", MeetingEvent { active });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(bundle: &str, name: &str, input: bool, output: bool) -> AudioProcess {
        AudioProcess { bundle: bundle.into(), name: name.into(), input, output }
    }

    fn running(bundle: &str, name: &str) -> RunningApp {
        RunningApp { bundle: bundle.into(), name: name.into() }
    }

    #[test]
    fn a_feishu_call_counts_even_muted_with_the_camera_off() {
        let activity = Activity {
            audio: vec![audio("com.electron.lark.iron", "Feishu Meetings", true, true)],
            running: vec![running("com.electron.lark", "Feishu")],
        };
        assert!(meeting_active(&activity));
        let listening_only = Activity {
            audio: vec![audio("com.electron.lark.iron", "Feishu Meetings", false, true)],
            running: vec![],
        };
        assert!(meeting_active(&listening_only));
    }

    #[test]
    fn a_browser_playing_audio_is_not_a_meeting() {
        let activity = Activity {
            audio: vec![
                audio("com.apple.WebKit.GPU", "Outsie Graphics and Media", false, true),
                audio("com.google.Chrome.helper", "Google Chrome Helper", false, true),
            ],
            running: vec![running("com.electron.lark", "Feishu")],
        };
        assert!(!meeting_active(&activity));
    }

    #[test]
    fn a_meeting_app_that_is_merely_open_is_not_a_meeting() {
        let activity = Activity {
            audio: vec![audio("com.electron.lark.iron", "Feishu Meetings", false, false)],
            running: vec![running("com.electron.lark", "Feishu"), running("us.zoom.xos", "zoom.us")],
        };
        assert!(!meeting_active(&activity));
    }

    #[test]
    fn zooms_meeting_helper_counts_without_audio() {
        let activity = Activity {
            audio: vec![],
            running: vec![running("us.zoom.xos", "zoom.us"), running("", "CptHost")],
        };
        assert!(meeting_active(&activity));
    }

    #[test]
    fn the_native_report_parses_with_bool_or_numeric_flags() {
        // The first device run shipped 0/1 here and the app never saw a meeting.
        let numeric = r#"{"audio":[{"bundle":"com.electron.lark.iron","input":1,"name":"Feishu Meetings","output":1}],"running":[]}"#;
        let parsed: Activity = serde_json::from_str(numeric).expect("numeric flags parse");
        assert!(meeting_active(&parsed));
        let boolean = r#"{"audio":[{"bundle":"com.electron.lark.iron","input":false,"name":"Feishu Meetings","output":true}],"running":[]}"#;
        let parsed: Activity = serde_json::from_str(boolean).expect("bool flags parse");
        assert!(meeting_active(&parsed));
        let empty: Activity = serde_json::from_str("{}").expect("empty object parses");
        assert!(!meeting_active(&empty));
    }

    #[test]
    fn every_known_meeting_app_matches_by_prefix() {
        for bundle in ["us.zoom.xos", "com.microsoft.teams2", "com.microsoft.teams2.helper",
            "com.electron.lark.iron", "com.larksuite.larkmac", "com.tencent.meeting", "com.apple.FaceTime"] {
            assert!(is_meeting_bundle(bundle), "{bundle}");
        }
        for bundle in ["com.apple.WebKit.GPU", "com.spotify.client", "", "ai.repose.outsie"] {
            assert!(!is_meeting_bundle(bundle), "{bundle}");
        }
    }

    #[test]
    fn the_answer_flips_only_after_five_agreeing_samples() {
        let mut debounce = Debounce::default();
        for _ in 0..4 {
            assert_eq!(debounce.observe(true), None);
        }
        assert_eq!(debounce.observe(true), Some(true));
        assert!(debounce.active());
        // A single quiet sample while joining does not end the meeting.
        assert_eq!(debounce.observe(false), None);
        assert_eq!(debounce.observe(true), None);
        assert!(debounce.active());
        // A streak is only consecutive samples: the interruption reset it.
        for _ in 0..4 {
            assert_eq!(debounce.observe(false), None);
        }
        assert_eq!(debounce.observe(false), Some(false));
        assert!(!debounce.active());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod live {
    use super::*;
    #[test]
    #[ignore]
    fn print_live_activity() {
        let raw = unsafe { repose_activity_json() };
        let text = unsafe { std::ffi::CStr::from_ptr(raw) }.to_string_lossy().into_owned();
        unsafe { repose_free_json(raw) };
        println!("RAW={}", &text[..text.len().min(600)]);
        let parsed: Result<Activity, _> = serde_json::from_str(&text);
        match parsed {
            Ok(activity) => println!("PARSED audio={} running={} active={}", activity.audio.len(), activity.running.len(), meeting_active(&activity)),
            Err(error) => println!("PARSE ERROR {error}"),
        }
    }
}
