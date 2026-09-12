//! `outsie shortcuts` — read and change the shortcut list from a shell, so a
//! person or an AI agent can configure 快捷控制 without the panel (design doc
//! §07). Everything here is pure: the binary in `bin/outsie.rs` only loads
//! the file, calls [apply], and writes the result back. The byte rules are
//! the panel's own -- an action keeps its byte for life, a new one gets a
//! fresh one -- because [crate::console::assign_cmd_bytes] is the only thing
//! that ever hands bytes out.

use crate::console::{assign_cmd_bytes, step_label, ConsoleAction, ConsoleApp, ConsoleConfig, ConsoleStep, CONSOLE_CMD_BASE};

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Every app and action, with keys and bytes.
    List { json: bool },
    /// Just the apps.
    Apps,
    AddApp { name: String, bundle: String, path: Option<String> },
    RemoveApp { name: String },
    /// One action in one app. `keys` is the human spelling, see [parse_keys].
    Add { app: String, name: String, keys: String, icon: Option<String> },
    Remove { app: String, name: String },
    /// Merge a JSON document (the same shape [Command::Export] prints, or the
    /// short one in [examples]).
    Import { json: String },
    Export,
    Schema,
    Examples,
    Help,
}

const MODIFIERS: &[(&str, &str)] = &[
    ("cmd", "cmd"), ("command", "cmd"), ("meta", "cmd"), ("⌘", "cmd"),
    ("ctrl", "ctrl"), ("control", "ctrl"), ("⌃", "ctrl"),
    ("alt", "alt"), ("option", "alt"), ("opt", "alt"), ("⌥", "alt"),
    ("shift", "shift"), ("⇧", "shift"),
];

const NAMED_KEYS: &[(&str, &str)] = &[
    ("enter", "Enter"), ("return", "Enter"), ("tab", "Tab"), ("space", "Space"),
    ("esc", "Escape"), ("escape", "Escape"), ("backspace", "Backspace"), ("delete", "Delete"),
    ("home", "Home"), ("end", "End"), ("pageup", "PageUp"), ("pagedown", "PageDown"),
    ("left", "ArrowLeft"), ("right", "ArrowRight"), ("up", "ArrowUp"), ("down", "ArrowDown"),
    ("arrowleft", "ArrowLeft"), ("arrowright", "ArrowRight"), ("arrowup", "ArrowUp"), ("arrowdown", "ArrowDown"),
];

/// `cmd+k`, `ctrl+b, %`, `shift+cmd+p`, `enter`. Steps are separated by
/// commas; within a step, `+` separates modifiers from the one key. A step
/// after the first gets a 100 ms breath, the way the panel records them.
pub fn parse_keys(spec: &str) -> Result<Vec<ConsoleStep>, String> {
    let mut steps = Vec::new();
    for (i, raw) in spec.split(',').enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("按键写法是「cmd+k」，几步用逗号隔开，例如「ctrl+b, %」。".into());
        }
        let parts: Vec<&str> = raw.split('+').map(str::trim).collect();
        // A literal "+" as the key: "cmd++" splits into ["cmd", "", ""]; a
        // trailing "+" alone ("cmd+") is a missing key, not a plus.
        let (mods, key): (Vec<&str>, String) = if parts.len() >= 3 && parts[parts.len() - 1].is_empty() && parts[parts.len() - 2].is_empty() {
            (parts[..parts.len() - 2].to_vec(), "+".into())
        } else {
            (parts[..parts.len() - 1].to_vec(), parts[parts.len() - 1].to_string())
        };
        let mut modifiers = Vec::new();
        for m in mods {
            let canon = MODIFIERS.iter().find(|(a, _)| a.eq_ignore_ascii_case(m)).map(|(_, c)| *c);
            match canon {
                Some(c) if !modifiers.iter().any(|x| x == c) => modifiers.push(c.to_string()),
                Some(_) => {}
                None => return Err(format!("「{m}」不是修饰键。能用的：cmd、ctrl、alt、shift。")),
            }
        }
        if key.is_empty() {
            return Err(format!("「{raw}」少了最后那个键。修饰键后面要跟一个键，例如「cmd+k」。"));
        }
        if MODIFIERS.iter().any(|(a, _)| a.eq_ignore_ascii_case(&key)) {
            return Err(format!("「{raw}」只有修饰键，没有要按的键。单独按修饰键还没支持。"));
        }
        let key = normalize_key(&key)?;
        steps.push(ConsoleStep { key, modifiers, delay_ms: if i == 0 { 0 } else { 100 } });
    }
    Ok(steps)
}

fn normalize_key(key: &str) -> Result<String, String> {
    if key.chars().count() == 1 {
        return Ok(key.to_string());
    }
    let lower = key.to_ascii_lowercase();
    if let Some((_, canon)) = NAMED_KEYS.iter().find(|(a, _)| *a == lower) {
        return Ok(canon.to_string());
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=20).contains(&n) {
            return Ok(format!("F{n}"));
        }
    }
    Err(format!("「{key}」这个键不认得。单个字符，或者 enter、tab、space、esc、up、down、left、right、f1…f20。"))
}

/// A stable id from a name: ASCII names become slugs, others a short hash.
/// Ids are what bytes follow across edits, so they must not change when the
/// name is merely retyped.
pub fn id_for(name: &str, taken: &[String]) -> String {
    let base: String = if name.is_ascii() {
        let s: String = name
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        s.trim_matches('-').to_string()
    } else {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("a-{:08x}", (h & 0xffff_ffff) as u32)
    };
    let base = if base.is_empty() { "a".to_string() } else { base };
    let mut id = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| *t == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn find_app_mut<'a>(config: &'a mut ConsoleConfig, name: &str) -> Result<&'a mut ConsoleApp, String> {
    config
        .apps
        .iter_mut()
        .find(|a| a.name == name)
        .ok_or_else(|| format!("没有叫「{name}」的 App。先 add-app，或者 apps 看看有哪些。"))
}

/// Do one thing to the config, and say what was done.
pub fn apply(config: &mut ConsoleConfig, cmd: &Command) -> Result<String, String> {
    let msg = match cmd {
        Command::AddApp { name, bundle, path } => {
            if config.apps.iter().any(|a| a.name == *name) {
                return Err(format!("「{name}」已经在了。要改它的按键，用 add；要换个 bundle，先 remove-app。"));
            }
            let taken: Vec<String> = config.apps.iter().map(|a| a.id.clone()).collect();
            config.apps.push(ConsoleApp {
                id: id_for(name, &taken),
                name: name.clone(),
                bundle_id: bundle.clone(),
                app_path: path.clone(),
                actions: Vec::new(),
                cmd_byte: None,
            });
            format!("加了「{name}」。手机上点它的名字，Mac 就切过去。")
        }
        Command::RemoveApp { name } => {
            let before = config.apps.len();
            config.apps.retain(|a| a.name != *name);
            if config.apps.len() == before {
                return Err(format!("没有叫「{name}」的 App。"));
            }
            format!("移掉了「{name}」和它的按钮。手机同步一次就看不到了。")
        }
        Command::Add { app, name, keys, icon } => {
            let steps = parse_keys(keys)?;
            let target = find_app_mut(config, app)?;
            if target.actions.iter().any(|a| a.name == *name) {
                return Err(format!("「{app}」里已经有「{name}」了。先 remove 再 add，或者换个名字。"));
            }
            let taken: Vec<String> = target.actions.iter().map(|a| a.id.clone()).collect();
            let kind = if steps.len() > 1 { "sequence" } else { "hotkey" }.to_string();
            let label = steps.iter().map(step_label).collect::<Vec<_>>().join(" ");
            target.actions.push(ConsoleAction {
                id: id_for(name, &taken),
                name: name.clone(),
                icon: icon.clone().filter(|s| !s.trim().is_empty()),
                kind,
                steps,
                cmd_byte: None,
            });
            format!("「{app}」里加了「{name}」：{label}。手机同步一次就有这个按钮。")
        }
        Command::Remove { app, name } => {
            let target = find_app_mut(config, app)?;
            let before = target.actions.len();
            target.actions.retain(|a| a.name != *name);
            if target.actions.len() == before {
                return Err(format!("「{app}」里没有叫「{name}」的操作。"));
            }
            format!("移掉了「{app}」里的「{name}」。")
        }
        Command::Import { json } => {
            let doc: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("这不是 JSON：{e}"))?;
            let apps = doc
                .get("apps")
                .and_then(|a| a.as_array())
                .ok_or("顶层要有 apps 数组。用 --examples 看格式。")?;
            let mut added_apps = 0;
            let mut added = 0;
            let mut replaced = 0;
            for a in apps {
                let name = a.get("name").and_then(|v| v.as_str()).ok_or("每个 App 要有 name。")?;
                let bundle = a.get("bundleId").and_then(|v| v.as_str());
                let path = a.get("appPath").and_then(|v| v.as_str()).map(str::to_string);
                if !config.apps.iter().any(|x| x.name == name) {
                    let bundle = bundle.ok_or_else(|| format!("新 App「{name}」要有 bundleId。"))?;
                    apply(config, &Command::AddApp { name: name.into(), bundle: bundle.into(), path })?;
                    added_apps += 1;
                }
                let target = find_app_mut(config, name)?;
                if let Some(b) = bundle {
                    target.bundle_id = b.to_string();
                }
                for x in a.get("actions").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                    let aname = x.get("name").and_then(|v| v.as_str()).ok_or("每个操作要有 name。")?;
                    let steps = match (x.get("keys").and_then(|v| v.as_str()), x.get("steps")) {
                        (Some(k), _) => parse_keys(k)?,
                        (None, Some(s)) => serde_json::from_value(s.clone()).map_err(|e| format!("「{aname}」的 steps 不对：{e}"))?,
                        (None, None) => return Err(format!("「{aname}」要有 keys（如「cmd+k」）或 steps。")),
                    };
                    let icon = x.get("icon").and_then(|v| v.as_str()).map(str::to_string);
                    let kind = if steps.len() > 1 { "sequence" } else { "hotkey" }.to_string();
                    if let Some(existing) = target.actions.iter_mut().find(|e| e.name == aname) {
                        // Same name: new keys, same id, same byte. A phone's
                        // list stays right about which button is which.
                        existing.steps = steps;
                        existing.kind = kind;
                        if icon.is_some() {
                            existing.icon = icon;
                        }
                        replaced += 1;
                    } else {
                        let taken: Vec<String> = target.actions.iter().map(|e| e.id.clone()).collect();
                        target.actions.push(ConsoleAction {
                            id: id_for(aname, &taken),
                            name: aname.into(),
                            icon,
                            kind,
                            steps,
                            cmd_byte: None,
                        });
                        added += 1;
                    }
                }
            }
            format!("导入了：新 App {added_apps} 个，新操作 {added} 个，改了按键的 {replaced} 个。手机同步一次。")
        }
        Command::List { .. } | Command::Apps | Command::Export | Command::Schema | Command::Examples | Command::Help => {
            return Err("这条只读，不改东西。".into());
        }
    };
    assign_cmd_bytes(config);
    config.revision = config.revision.wrapping_add(1);
    Ok(msg)
}

/// Whether a command changes the file.
pub fn writes(cmd: &Command) -> bool {
    !matches!(cmd, Command::List { .. } | Command::Apps | Command::Export | Command::Schema | Command::Examples | Command::Help)
}

pub fn render(config: &ConsoleConfig, cmd: &Command) -> String {
    match cmd {
        Command::List { json: true } | Command::Export => export_json(config),
        Command::List { json: false } => {
            let mut out = String::new();
            for app in &config.apps {
                out.push_str(&format!("{}  ({}){}\n", app.name, app.bundle_id, byte_note(app.cmd_byte)));
                for a in &app.actions {
                    let keys = a.steps.iter().map(step_label).collect::<Vec<_>>().join(" ");
                    out.push_str(&format!("  {:<12} {}{}\n", a.name, keys, byte_note(a.cmd_byte)));
                }
            }
            if out.is_empty() {
                out.push_str("还没有任何 App。add-app 加一个。\n");
            }
            out
        }
        Command::Apps => {
            let mut out = String::new();
            for app in &config.apps {
                out.push_str(&format!("{}  ({})  {} 个操作\n", app.name, app.bundle_id, app.actions.len()));
            }
            if out.is_empty() {
                out.push_str("还没有任何 App。\n");
            }
            out
        }
        Command::Schema => SCHEMA.to_string(),
        Command::Examples => EXAMPLES.to_string(),
        Command::Help => HELP.to_string(),
        _ => String::new(),
    }
}

fn byte_note(b: Option<u8>) -> String {
    match b {
        Some(b) if b >= CONSOLE_CMD_BASE => format!("   #{b}"),
        _ => "   (还没分到字节)".into(),
    }
}

/// The document `import` reads back: names and keys in the human spelling,
/// plus the ids and bytes so a round trip changes nothing.
pub fn export_json(config: &ConsoleConfig) -> String {
    let apps: Vec<serde_json::Value> = config
        .apps
        .iter()
        .map(|app| {
            serde_json::json!({
                "name": app.name,
                "bundleId": app.bundle_id,
                "appPath": app.app_path,
                "cmdByte": app.cmd_byte,
                "actions": app.actions.iter().map(|a| serde_json::json!({
                    "name": a.name,
                    "icon": a.icon,
                    "keys": a.steps.iter().map(|s| {
                        let mut parts: Vec<String> = s.modifiers.clone();
                        parts.push(s.key.clone());
                        parts.join("+")
                    }).collect::<Vec<_>>().join(", "),
                    "cmdByte": a.cmd_byte,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({ "revision": config.revision, "apps": apps })).unwrap_or_default()
}

pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter().map(String::as_str);
    let sub = it.next().unwrap_or("help");
    let rest: Vec<&str> = it.collect();
    let opt = |name: &str| -> Option<String> {
        rest.iter().position(|a| *a == name).and_then(|i| rest.get(i + 1)).map(|s| s.to_string())
    };
    let flag = |name: &str| rest.iter().any(|a| *a == name);
    let need = |name: &str| opt(name).ok_or_else(|| format!("少了 {name}。看 help。"));
    Ok(match sub {
        "list" | "ls" => Command::List { json: flag("--json") },
        "apps" => Command::Apps,
        "add-app" => Command::AddApp { name: need("--name")?, bundle: need("--bundle")?, path: opt("--path") },
        "remove-app" => Command::RemoveApp { name: need("--name")? },
        "add" => Command::Add { app: need("--app")?, name: need("--name")?, keys: need("--keys")?, icon: opt("--icon") },
        "remove" | "rm" => Command::Remove { app: need("--app")?, name: need("--name")? },
        "import" => Command::Import { json: rest.first().map(|s| s.to_string()).ok_or("import 后面跟 JSON 文件的路径，或 - 读标准输入。")? },
        "export" => Command::Export,
        "--schema" | "schema" => Command::Schema,
        "--examples" | "examples" => Command::Examples,
        "help" | "--help" | "-h" => Command::Help,
        other => return Err(format!("不认得「{other}」。看 help。")),
    })
}

pub const HELP: &str = "outsie shortcuts — 从命令行看和改快捷控制的列表

  outsie shortcuts list [--json]                    每个 App 和它的操作、按键、字节
  outsie shortcuts apps                             只看 App
  outsie shortcuts add-app --name 飞书 --bundle com.electron.lark [--path /Applications/Lark.app]
  outsie shortcuts remove-app --name 飞书
  outsie shortcuts add --app 飞书 --name 搜索 --keys \"cmd+k\" [--icon ⌕]
  outsie shortcuts remove --app 飞书 --name 搜索
  outsie shortcuts import 文件.json | -              合并进去（同名 App 更新，同名操作换按键，字节不变）
  outsie shortcuts export                           打印可以再 import 的 JSON
  outsie shortcuts --schema | --examples            给 AI 看的格式和例子

按键写法：cmd+k · shift+cmd+p · ctrl+b, %（几步用逗号隔开）· enter · f12 · up
改完 Mac 立刻读到；手机上按一次「同步」。
";

pub const SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "outsie shortcuts import",
  "type": "object",
  "required": ["apps"],
  "properties": {
    "apps": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["name"],
        "properties": {
          "name": { "type": "string", "description": "shown on the phone; the key for merging" },
          "bundleId": { "type": "string", "description": "required for a new App, e.g. com.electron.lark" },
          "appPath": { "type": "string", "description": "optional, e.g. /Applications/Lark.app" },
          "actions": {
            "type": "array",
            "items": {
              "type": "object",
              "required": ["name"],
              "properties": {
                "name": { "type": "string", "description": "button label; merging key inside the App" },
                "keys": { "type": "string", "description": "cmd+k · shift+cmd+p · ctrl+b, % (steps separated by commas) · enter · f12 · up" },
                "icon": { "type": "string", "description": "one glyph, optional" }
              }
            }
          }
        }
      }
    }
  }
}
"#;

pub const EXAMPLES: &str = r#"# 看看现在有什么
outsie shortcuts list

# 给飞书加两个按钮
outsie shortcuts add --app 飞书 --name 搜索 --keys "cmd+k" --icon ⌕
outsie shortcuts add --app 飞书 --name 发送 --keys "enter"

# 一次导入一批（同名 App 更新，同名操作只换按键，按钮的字节不变）
cat > /tmp/s.json <<'EOF'
{ "apps": [
  { "name": "Arc", "bundleId": "company.thebrowser.Browser", "actions": [
    { "name": "新标签", "keys": "cmd+t", "icon": "＋" },
    { "name": "下一空间", "keys": "alt+cmd+right" } ] },
  { "name": "Otty", "bundleId": "io.appmakes.otty", "actions": [
    { "name": "左右分屏", "keys": "ctrl+b, %", "icon": "◫" } ] }
] }
EOF
outsie shortcuts import /tmp/s.json

# 手机上按一次「同步」就看得到。
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ConsoleConfig {
        let mut c = ConsoleConfig { revision: 3, apps: vec![] };
        apply(&mut c, &Command::AddApp { name: "飞书".into(), bundle: "com.electron.lark".into(), path: None }).unwrap();
        apply(&mut c, &Command::Add { app: "飞书".into(), name: "搜索".into(), keys: "cmd+k".into(), icon: Some("⌕".into()) }).unwrap();
        c
    }

    #[test]
    fn keys_are_spelled_the_way_people_say_them() {
        let s = parse_keys("shift+cmd+p").unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].key, "p");
        assert_eq!(s[0].modifiers, vec!["shift", "cmd"]);
        let seq = parse_keys("ctrl+b, %").unwrap();
        assert_eq!(seq.len(), 2);
        assert_eq!(seq[0].modifiers, vec!["ctrl"]);
        assert_eq!(seq[1].key, "%");
        assert_eq!(seq[1].delay_ms, 100, "a breath between steps, like the panel");
        assert_eq!(parse_keys("Enter").unwrap()[0].key, "Enter");
        assert_eq!(parse_keys("f12").unwrap()[0].key, "F12");
        assert_eq!(parse_keys("alt+cmd+right").unwrap()[0].key, "ArrowRight");
        assert_eq!(parse_keys("cmd++").unwrap()[0].key, "+");
        assert_eq!(parse_keys("⌘+⇧+p").unwrap()[0].modifiers, vec!["cmd", "shift"]);
    }

    #[test]
    fn a_bad_spelling_says_what_is_wrong_in_the_users_words() {
        assert!(parse_keys("cmd+").unwrap_err().contains("少了最后那个键"));
        assert!(parse_keys("win+k").unwrap_err().contains("不是修饰键"));
        assert!(parse_keys("cmd").unwrap_err().contains("只有修饰键"));
        assert!(parse_keys("cmd+fooey").unwrap_err().contains("不认得"));
        assert!(parse_keys("").unwrap_err().contains("cmd+k"));
    }

    #[test]
    fn adding_gives_a_byte_and_a_stable_id_and_bumps_the_revision() {
        let c = cfg();
        assert_eq!(c.revision, 5, "two writes, two bumps");
        let app = &c.apps[0];
        assert!(app.cmd_byte.unwrap() >= CONSOLE_CMD_BASE);
        let a = &app.actions[0];
        assert!(a.cmd_byte.unwrap() >= CONSOLE_CMD_BASE);
        assert_ne!(a.cmd_byte, app.cmd_byte);
        assert_eq!(a.kind, "hotkey");
        assert_eq!(a.icon.as_deref(), Some("⌕"));
        assert!(a.id.starts_with("a-"), "a non-ASCII name gets a hashed id: {}", a.id);
        assert_eq!(id_for("搜索", &[]), id_for("搜索", &[]), "same name, same id");
        assert_eq!(id_for("New Tab", &[]), "new-tab");
        assert_eq!(id_for("new-tab", &["new-tab".into()]), "new-tab-2");
    }

    #[test]
    fn the_same_name_twice_is_refused_rather_than_doubled() {
        let mut c = cfg();
        let err = apply(&mut c, &Command::Add { app: "飞书".into(), name: "搜索".into(), keys: "cmd+f".into(), icon: None }).unwrap_err();
        assert!(err.contains("已经有"));
        let err = apply(&mut c, &Command::AddApp { name: "飞书".into(), bundle: "x".into(), path: None }).unwrap_err();
        assert!(err.contains("已经在了"));
        let err = apply(&mut c, &Command::Add { app: "没有的".into(), name: "x".into(), keys: "cmd+k".into(), icon: None }).unwrap_err();
        assert!(err.contains("没有叫"));
    }

    #[test]
    fn import_updates_by_name_and_keeps_bytes() {
        let mut c = cfg();
        let byte = c.apps[0].actions[0].cmd_byte;
        let msg = apply(&mut c, &Command::Import { json: r#"{"apps":[
            {"name":"飞书","actions":[{"name":"搜索","keys":"cmd+shift+k"},{"name":"发送","keys":"enter"}]},
            {"name":"Arc","bundleId":"company.thebrowser.Browser","actions":[{"name":"新标签","keys":"cmd+t"}]}
        ]}"#.into() }).unwrap();
        assert!(msg.contains("新 App 1"), "{msg}");
        let lark = &c.apps[0];
        assert_eq!(lark.actions[0].cmd_byte, byte, "a renamed key keeps its byte");
        assert_eq!(lark.actions[0].steps[0].modifiers, vec!["cmd", "shift"]);
        assert_eq!(lark.actions.len(), 2);
        assert_eq!(c.apps[1].name, "Arc");
        assert!(c.apps[1].actions[0].cmd_byte.is_some());
        // Every byte unique across apps and actions.
        let mut all: Vec<u8> = c.apps.iter().flat_map(|a| std::iter::once(a.cmd_byte).chain(a.actions.iter().map(|x| x.cmd_byte))).flatten().collect();
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), n);
    }

    #[test]
    fn import_refuses_what_it_cannot_place() {
        let mut c = cfg();
        assert!(apply(&mut c, &Command::Import { json: "not json".into() }).unwrap_err().contains("不是 JSON"));
        assert!(apply(&mut c, &Command::Import { json: r#"{"apps":[{"name":"新的","actions":[]}]}"#.into() }).unwrap_err().contains("bundleId"));
        assert!(apply(&mut c, &Command::Import { json: r#"{"apps":[{"name":"飞书","actions":[{"name":"x"}]}]}"#.into() }).unwrap_err().contains("keys"));
    }

    #[test]
    fn export_round_trips_through_import_without_moving_a_byte() {
        let mut c = cfg();
        let before = c.clone();
        let json = export_json(&c);
        apply(&mut c, &Command::Import { json }).unwrap();
        assert_eq!(c.apps, before.apps);
    }

    #[test]
    fn remove_takes_the_action_and_leaves_the_others_bytes_alone() {
        let mut c = cfg();
        apply(&mut c, &Command::Add { app: "飞书".into(), name: "发送".into(), keys: "enter".into(), icon: None }).unwrap();
        let keep = c.apps[0].actions[1].cmd_byte;
        apply(&mut c, &Command::Remove { app: "飞书".into(), name: "搜索".into() }).unwrap();
        assert_eq!(c.apps[0].actions.len(), 1);
        assert_eq!(c.apps[0].actions[0].cmd_byte, keep);
        apply(&mut c, &Command::RemoveApp { name: "飞书".into() }).unwrap();
        assert!(c.apps.is_empty());
    }

    #[test]
    fn the_listing_reads_like_the_panel_and_shows_bytes() {
        let c = cfg();
        let out = render(&c, &Command::List { json: false });
        assert!(out.contains("飞书  (com.electron.lark)   #"), "{out}");
        assert!(out.contains("搜索"), "{out}");
        assert!(out.contains("⌘k"), "{out}");
        assert!(render(&c, &Command::Apps).contains("1 个操作"));
        assert!(render(&c, &Command::Schema).contains("\"apps\""));
        assert!(render(&c, &Command::Examples).contains("import"));
        assert!(render(&c, &Command::Help).contains("outsie shortcuts add"));
    }

    #[test]
    fn arguments_parse_the_documented_way() {
        let a = |s: &str| parse_args(&s.split_whitespace().map(String::from).collect::<Vec<_>>());
        assert_eq!(a("list --json").unwrap(), Command::List { json: true });
        assert_eq!(a("add --app 飞书 --name 搜索 --keys cmd+k").unwrap(),
            Command::Add { app: "飞书".into(), name: "搜索".into(), keys: "cmd+k".into(), icon: None });
        assert!(a("add --app 飞书").unwrap_err().contains("--name"));
        assert!(a("frobnicate").unwrap_err().contains("不认得"));
        assert!(writes(&Command::Export) == false && writes(&Command::Remove { app: "a".into(), name: "b".into() }));
    }
}
