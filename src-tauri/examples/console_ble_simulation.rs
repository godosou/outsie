//! Hardware-free Kotlin/Rust BLE protocol simulation. Debug only; uses no Mac keyboard.
#[cfg(not(debug_assertions))]
fn main() {
    eprintln!("Simulation is only available in debug builds");
    std::process::exit(1);
}
#[cfg(debug_assertions)]
fn main() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use rand_core::{OsRng, RngCore};
    use repose_lib::{
        console_bluetooth::{self, BleSession},
        work_console::{Keyboard, Step, WorkConsole},
    };
    use serde_json::{Value, json};
    use std::{
        io::{self, BufRead, Write},
        sync::{Arc, Mutex},
        time::Instant,
    };
    struct FakeKeyboard {
        events: Mutex<Vec<Value>>,
        start: Instant,
    }
    impl Keyboard for FakeKeyboard {
        fn trusted(&self, _: bool) -> bool {
            true
        }
        fn activate(
            &self,
            app: &repose_lib::work_console::ConsoleApp,
            valid: &(dyn Fn() -> bool + Sync),
        ) -> Result<(), String> {
            if !valid() {
                return Err("cancelled".into());
            }
            self.events
                .lock()
                .unwrap()
                .push(json!({"activate":app.bundle_id}));
            Ok(())
        }
        fn send(
            &self,
            _: &repose_lib::work_console::ConsoleApp,
            step: &Step,
            valid: &(dyn Fn() -> bool + Sync),
        ) -> Result<(), String> {
            if !valid() {
                return Err("cancelled".into());
            }
            self.events.lock().unwrap().push(json!({"key":step.key,"modifiers":step.modifiers,"atMs":self.start.elapsed().as_millis()}));
            Ok(())
        }
    }
    let path =
        std::env::temp_dir().join(format!("repose-ble-simulation-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let keyboard = Arc::new(FakeKeyboard {
        events: Mutex::new(vec![]),
        start: Instant::now(),
    });
    let console = WorkConsole::new(path.clone(), keyboard.clone(), Arc::new(|| false));
    let mut config = console.status().config;
    config.apps[0].actions[0].steps[1].delay_ms = 500;
    console.save(config).expect("simulation config");
    console_bluetooth::authorize_simulation();
    let mut epoch = console.start_simulation();
    let mut channel: Option<BleSession> = None;
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.len() > 400_000 {
            break;
        }
        let result = (|| -> Result<Value, String> {
            let input: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            match input["event"].as_str() {
                Some("subscribe") => {
                    let mut challenge = [0; 16];
                    OsRng.fill_bytes(&mut challenge);
                    channel = Some(BleSession::new(challenge));
                    epoch = console.start_simulation();
                    Ok(json!({"event":"challenge","data":STANDARD.encode(challenge)}))
                }
                Some("frame") => {
                    let packet = STANDARD
                        .decode(input["data"].as_str().ok_or("missing data")?)
                        .map_err(|e| e.to_string())?;
                    let service = console.clone();
                    let encrypted = channel.as_mut().ok_or("not subscribed")?.process(
                        &packet,
                        &move |value, generation| service.ble_request(epoch, value, generation),
                        console_bluetooth::link_generation(),
                    )?;
                    Ok(json!({"event":"response","data":STANDARD.encode(encrypted)}))
                }
                Some("state") => Ok(
                    json!({"event":"state","running":console.status().running,"events":*keyboard.events.lock().unwrap()}),
                ),
                Some("disconnect") => {
                    channel = None;
                    console.stop();
                    Ok(json!({"event":"disconnected"}))
                }
                Some("revoke") => {
                    console_bluetooth::revoke_device("simulated-phone");
                    Ok(json!({"event":"revoked"}))
                }
                Some("authorize") => {
                    console_bluetooth::authorize_simulation();
                    Ok(json!({"event":"authorized"}))
                }
                _ => Err("unknown simulation event".into()),
            }
        })();
        let output = result.unwrap_or_else(|error| json!({"event":"error","error":error}));
        println!("{output}");
        io::stdout().flush().unwrap();
    }
    console.stop();
    let _ = std::fs::remove_file(path);
}
