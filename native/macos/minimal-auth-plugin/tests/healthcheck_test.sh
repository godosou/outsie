#!/bin/bash
#
# Sandbox tests for healthcheck.sh (E12). No root, no real authorization
# database, no VM: a fake authdb (two plist files on disk), a fake codesign, and
# a temp SecurityAgentPlugins directory, all injected through the REPOSE_HC_*
# environment variables healthcheck.sh already exposes.
#
# The point is to pin the two things that make this safe to run unattended as
# root:
#   - it repairs (removes the sub-rule) ONLY when the reference is truly
#     dangling or the bundle is truly unloadable, and
#   - it NEVER writes when it cannot read the database, and never removes the
#     last password path (authdb-edit's refusal must hold through it).

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HC="${HERE}/../healthcheck.sh"

pass=0; fail=0
ok()   { printf '  ok   %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  FAIL %s\n' "$1"; fail=$((fail+1)); }

SANDBOX=""
cleanup() { [ -n "${SANDBOX}" ] && rm -rf "${SANDBOX}"; }
trap cleanup EXIT

# A fresh sandbox: fake authdb dir, fake tools, temp plugins dir, state dir.
new_sandbox() {
    SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/hc-test.XXXXXX")"
    DB="${SANDBOX}/db";           mkdir -p "${DB}"
    PLUGINS="${SANDBOX}/plugins"; mkdir -p "${PLUGINS}"
    STATE="${SANDBOX}/state";     mkdir -p "${STATE}"

    cat > "${SANDBOX}/fakeauthdb" <<'SH'
#!/bin/bash
# $1 = read|write ; $2 = right name. Fixtures live as <name>.plist in $DB.
case "$1" in
  read)  cat "${DB}/$2.plist" 2>/dev/null ;;
  write) cat > "${DB}/$2.plist" ;;
esac
SH
    chmod +x "${SANDBOX}/fakeauthdb"

    cat > "${SANDBOX}/fakecodesign" <<'SH'
#!/bin/bash
# mimic `codesign --verify [flags] <bundle>`: last arg is the bundle. Passes
# only if the bundle carries a .codesign-ok marker.
bundle="${@: -1}"
[ -f "${bundle}/.codesign-ok" ]
SH
    chmod +x "${SANDBOX}/fakecodesign"
}

# Write the screensaver rule fixture. $1 = "referenced" | "clean" | "only".
put_screensaver() {
    local body
    case "$1" in
        referenced) body='<string>ai.repose.spike</string><string>use-login-window-ui</string>' ;;
        clean)      body='<string>use-login-window-ui</string>' ;;
        only)       body='<string>ai.repose.spike</string>' ;;  # no fallback: brick-risk
    esac
    cat > "${DB}/system.login.screensaver.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>class</key><string>rule</string>
<key>k-of-n</key><integer>1</integer>
<key>rule</key><array>${body}</array>
<key>version</key><integer>1</integer>
</dict></plist>
EOF
}

put_subrule() {
    cat > "${DB}/ai.repose.spike.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>class</key><string>evaluate-mechanisms</string>
<key>mechanisms</key><array><string>ReposeSpike:permit</string></array>
<key>version</key><integer>0</integer>
</dict></plist>
EOF
}

make_bundle()      { mkdir -p "${PLUGINS}/ReposeSpike.bundle"; touch "${PLUGINS}/ReposeSpike.bundle/.codesign-ok"; }
make_bundle_unsigned() { mkdir -p "${PLUGINS}/ReposeSpike.bundle"; rm -f "${PLUGINS}/ReposeSpike.bundle/.codesign-ok"; }

run_hc() {
    REPOSE_HC_READ_CMD="${SANDBOX}/fakeauthdb read" \
    REPOSE_HC_WRITE_CMD="${SANDBOX}/fakeauthdb write" \
    REPOSE_HC_CODESIGN_CMD="${SANDBOX}/fakecodesign --verify" \
    REPOSE_HC_PLUGINS_DIR="${PLUGINS}" \
    REPOSE_HC_STATE_DIR="${STATE}" \
    DB="${DB}" \
        bash "${HC}" >/dev/null 2>&1
}

rule_has() { grep -q "$1" "${DB}/system.login.screensaver.plist" 2>/dev/null; }

echo "healthcheck.sh (E12)"

# 1. Healthy: referenced, bundle present and signed -> no change.
new_sandbox; put_screensaver referenced; put_subrule; make_bundle
run_hc; rc=$?
{ [ "$rc" = 0 ] && rule_has "ai.repose.spike" && rule_has "use-login-window-ui"; } \
    && ok "healthy install is left untouched" \
    || bad "healthy install should be untouched (rc=$rc)"

# 2. Dangling: bundle missing -> repaired to password-only.
new_sandbox; put_screensaver referenced; put_subrule   # no make_bundle
run_hc; rc=$?
{ [ "$rc" = 0 ] && ! rule_has "ai.repose.spike" && rule_has "use-login-window-ui"; } \
    && ok "missing bundle -> sub-rule removed, password path kept" \
    || bad "missing bundle should be repaired to password-only (rc=$rc)"

# 3. Not referenced: nothing to do, unchanged.
new_sandbox; put_screensaver clean; put_subrule; make_bundle
run_hc; rc=$?
{ [ "$rc" = 0 ] && ! rule_has "ai.repose.spike" && rule_has "use-login-window-ui"; } \
    && ok "rule that never referenced us is left alone" \
    || bad "unreferenced rule should be untouched (rc=$rc)"

# 4. Unreadable authdb: read yields nothing -> do NOTHING, no write.
new_sandbox; put_subrule; make_bundle          # no screensaver fixture at all
run_hc; rc=$?
{ [ "$rc" = 0 ] && [ ! -f "${DB}/system.login.screensaver.plist" ]; } \
    && ok "unreadable rule -> no write, no guess" \
    || bad "unreadable rule must not be written (rc=$rc)"

# 5. Bundle present but codesign fails -> treated as unloadable, repaired.
new_sandbox; put_screensaver referenced; put_subrule; make_bundle_unsigned
run_hc; rc=$?
{ [ "$rc" = 0 ] && ! rule_has "ai.repose.spike"; } \
    && ok "bundle failing codesign -> repaired" \
    || bad "unloadable bundle should be repaired (rc=$rc)"

# 6. Sub-rule referenced but its mechanisms are gone -> dangling, repaired.
new_sandbox; put_screensaver referenced; make_bundle   # no put_subrule
run_hc; rc=$?
{ [ "$rc" = 0 ] && ! rule_has "ai.repose.spike"; } \
    && ok "referenced sub-rule with no readable mechanisms -> repaired" \
    || bad "unreadable sub-rule should be treated as dangling (rc=$rc)"

# 7. Brick-guard: dangling, but removing us would leave no password path ->
#    authdb-edit refuses, repair aborts, rule left as-is (non-zero exit).
new_sandbox; put_screensaver only; put_subrule   # no bundle -> dangling; no fallback in rule
run_hc; rc=$?
{ [ "$rc" != 0 ] && rule_has "ai.repose.spike"; } \
    && ok "refuses to remove the last entry (no lockout), signals failure" \
    || bad "should refuse and keep the rule when no fallback exists (rc=$rc)"

# 8. bundle_names parsing excludes macOS mechanism hosts.
# shellcheck disable=SC1090
source "${HC}"
names="$(bundle_names '["builtin:authenticate","loginwindow:done","ReposeSpike:permit","builtin:reset-password,privileged"]')"
{ [ "${names}" = "ReposeSpike" ]; } \
    && ok "bundle_names keeps ReposeSpike, drops builtin/loginwindow" \
    || bad "bundle_names parsing wrong: got [${names}]"

echo
echo "${pass} passed, ${fail} failed"
[ "${fail}" = 0 ]
