//! `outsie` — the command line beside the app. Today one thing lives here:
//! `outsie shortcuts`, which reads and changes the 快捷控制 list the panel
//! edits (design doc §07). The file is the same one the app reads, so a
//! change here shows in 快捷键设置 at once and on the phone after 同步.

use repose_lib::console::{ConsoleConfig, CONSOLE_FILE};
use repose_lib::console_cli::{apply, parse_args, render, writes, Command, HELP};
use std::io::Read;
use std::path::PathBuf;

fn config_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("找不到 HOME。")?;
    let dir = PathBuf::from(home).join("Library/Application Support/ai.repose.lite");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建不了 {}：{e}", dir.display()))?;
    Ok(dir.join(CONSOLE_FILE))
}

fn load(path: &PathBuf) -> Result<ConsoleConfig, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{} 读不出来：{e}。不动它，先备份再说。", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConsoleConfig::default()),
        Err(e) => Err(format!("读不了 {}：{e}", path.display())),
    }
}

fn save(path: &PathBuf, config: &ConsoleConfig) -> Result<(), String> {
    let body = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.writing");
    std::fs::write(&tmp, body).map_err(|e| format!("写不了 {}：{e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("换不上 {}：{e}", path.display()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `outsie shortcuts …` today; `outsie …` alone prints the same help.
    let rest: Vec<String> = match args.first().map(String::as_str) {
        Some("shortcuts") => args[1..].to_vec(),
        Some("help") | Some("--help") | Some("-h") | None => vec!["help".into()],
        Some(other) => {
            eprintln!("不认得「{other}」。现在只有 outsie shortcuts。\n\n{HELP}");
            std::process::exit(2);
        }
    };
    let mut cmd = match parse_args(&rest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    // `import` takes a path or `-`; the pure layer wants the JSON itself.
    if let Command::Import { json } = &cmd {
        let text = if json == "-" {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).unwrap_or(0);
            s
        } else {
            match std::fs::read_to_string(json) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("读不了 {json}：{e}");
                    std::process::exit(2);
                }
            }
        };
        cmd = Command::Import { json: text };
    }
    let path = match config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mut config = match load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if writes(&cmd) {
        match apply(&mut config, &cmd) {
            Ok(msg) => {
                if let Err(e) = save(&path, &config) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                println!("{msg}");
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    } else {
        print!("{}", render(&config, &cmd));
    }
}
