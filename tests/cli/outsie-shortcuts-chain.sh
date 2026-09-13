#!/bin/sh
# The 让 AI 帮你配 chain, end to end (design doc §07):
#
#   prompt (what the panel copies)  ->  an AI's answer  ->  outsie shortcuts import
#   ->  the file the Mac reads  ->  what the phone will get after 同步
#
# Runs against a throwaway HOME so the real configuration is never touched.
# By default the "AI" is a canned answer that follows the prompt's rules; with
# --live it asks `claude -p` for real and imports whatever comes back, which
# is the only honest test of the prompt itself.
#
#   sh tests/cli/outsie-shortcuts-chain.sh          # canned answer
#   sh tests/cli/outsie-shortcuts-chain.sh --live   # a real model
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
cli="${OUTSIE_CLI:-$root/src-tauri/target/release/outsie-cli}"
[ -x "$cli" ] || { echo "build the CLI first: cargo build --release --bin outsie-cli --manifest-path src-tauri/Cargo.toml"; exit 2; }

real_home="$HOME"
home="$(mktemp -d)"
trap 'rm -rf "$home"' EXIT
export HOME="$home"
mkdir -p "$home/Library/Application Support/ai.repose.lite"
cat > "$home/Library/Application Support/ai.repose.lite/work-console-v1.json" <<'EOF'
{ "revision": 3, "apps": [
  { "id": "lark", "name": "飞书", "bundleId": "com.electron.lark", "cmdByte": 105,
    "actions": [ { "id": "search", "name": "搜索", "kind": "hotkey", "steps": [ { "key": "k", "modifiers": ["cmd"], "delayMs": 0 } ], "cmdByte": 100 } ] } ] }
EOF

fail() { echo "FAIL: $*"; exit 1; }

# 1. The prompt is what the panel copies; it must name the command and the format.
prompt="$("$cli" shortcuts --prompt)"
echo "$prompt" | grep -q "outsie shortcuts import" || fail "prompt does not name the import command"
echo "$prompt" | grep -q '"keys"' || fail "prompt does not show the keys field"
echo "$prompt" | grep -q "飞书" || fail "prompt does not list what is already there"

# 2. The AI's answer: a JSON document in the prompt's format.
answer="$home/answer.json"
if [ "${1:-}" = "--live" ]; then
  command -v claude >/dev/null || fail "--live needs the claude CLI"
  ask="$prompt

任务：给「飞书」加两个按钮——「回到顶部」是 cmd+up，「发送」是 enter。再加一个新 App「访达」，bundleId 是 com.apple.finder，给它一个「新建窗口」cmd+n。
只回答一份可以直接交给 outsie shortcuts import 的 JSON，不要别的文字，不要代码块标记。"
  # The model runs with the real HOME (its login lives there); only the CLI
  # under test sees the throwaway one.
  HOME="$real_home" claude -p "$ask" --output-format text > "$answer" 2>"$home/claude.err" || fail "claude -p failed: $(head -c 300 "$home/claude.err")"
  # Models sometimes wrap JSON in a fence despite being asked not to.
  sed -i '' -e 's/^```json$//' -e 's/^```$//' "$answer"
  echo "--- the model answered:"; cat "$answer"; echo "---"
else
  cat > "$answer" <<'EOF'
{ "apps": [
  { "name": "飞书", "actions": [ { "name": "回到顶部", "keys": "cmd+up" }, { "name": "发送", "keys": "enter" } ] },
  { "name": "访达", "bundleId": "com.apple.finder", "actions": [ { "name": "新建窗口", "keys": "cmd+n" } ] }
] }
EOF
fi

# 3. Import it the way the AI would.
"$cli" shortcuts import "$answer" | grep -q "导入了" || fail "import did not report success"

# 4. What the Mac now holds, as the phone will receive it.
out="$("$cli" shortcuts list --json)"
echo "$out" | grep -q '"name": "回到顶部"' || fail "回到顶部 missing"
echo "$out" | grep -q '"keys": "cmd+ArrowUp"' || fail "回到顶部 keys wrong: $(echo "$out" | grep -A1 回到顶部 | tail -1)"
echo "$out" | grep -q '"name": "访达"' || fail "访达 missing"
echo "$out" | grep -q '"bundleId": "com.apple.finder"' || fail "访达 bundle missing"
# Byte stability: the button that was there keeps its byte; every byte is unique.
echo "$out" | python3 -c '
import json,sys
d=json.load(sys.stdin)
lark=[a for a in d["apps"] if a["name"]=="飞书"][0]
assert lark["cmdByte"]==105, lark["cmdByte"]
assert [x for x in lark["actions"] if x["name"]=="搜索"][0]["cmdByte"]==100
bytes_=[a["cmdByte"] for a in d["apps"]]+[x["cmdByte"] for a in d["apps"] for x in a["actions"]]
assert None not in bytes_, bytes_
assert len(bytes_)==len(set(bytes_)), bytes_
print("bytes ok:", sorted(bytes_))
' || fail "byte rules broken"
echo "PASS: prompt -> AI -> import -> list, revision $(echo "$out" | python3 -c 'import json,sys;print(json.load(sys.stdin)["revision"])')"
