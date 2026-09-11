#!/bin/bash
#
# Sandbox tests for permit-bridge.sh. No radio, no VM: a timed producer feeds
# synthetic RSSI CSV, and the permit assert/clear commands are pointed at a log
# file, so every hysteresis / staleness / refresh transition is checked here.
#
# The producer keeps its stdout open across `sleep`s so the bridge's read times
# out (exercising the timer paths) instead of hitting EOF.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BRIDGE="${HERE}/permit-bridge.sh"

pass=0; fail=0
ok()  { printf '  ok   %s\n' "$1"; pass=$((pass+1)); }
bad() { printf '  FAIL %s -- %s\n' "$1" "${2:-}"; fail=$((fail+1)); }

SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/pbridge.XXXXXX")"
trap 'rm -rf "${SANDBOX}"' EXIT
ACTIONS="${SANDBOX}/actions.log"

# Run the bridge with a given producer function; capture stderr and the ON/OFF
# action log. $1 = producer function name.
run_case() {
    : > "${ACTIONS}"
    local producer="$1"
    STDERR="${SANDBOX}/stderr.log"
    "${producer}" | REPOSE_NEAR_DBM=-72 REPOSE_FAR_DBM=-85 \
        REPOSE_STALE_S=2 REPOSE_REFRESH_S=1 \
        REPOSE_PERMIT_ON_CMD="printf 'ON\n' >> '${ACTIONS}'" \
        REPOSE_PERMIT_OFF_CMD="printf 'OFF\n' >> '${ACTIONS}'" \
        bash "${BRIDGE}" 2> "${STDERR}"
}
saw()   { grep -q "$1" "${STDERR}"; }
# grep -c prints 0 AND exits 1 when nothing matches, so a `|| echo 0` fallback
# emits "0\n0" -- which every `-ge 1` assertion swallows and every `= 0`
# assertion fails on. Count with grep -c alone and let a missing file be 0.
on_count()  { [ -f "${ACTIONS}" ] && grep -c '^ON$'  "${ACTIONS}" 2>/dev/null; true; }
off_count() { [ -f "${ACTIONS}" ] && grep -c '^OFF$' "${ACTIONS}" 2>/dev/null; true; }

echo "permit-bridge.sh"

# 1. A near sample enters and asserts the permit; EOF then clears it.
p_enter() { printf '0,-60,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 0.3; }
run_case p_enter
{ saw ENTER && [ "$(on_count)" -ge 1 ] && [ "$(off_count)" -ge 1 ]; } \
    && ok "near sample enters (asserts permit), stream end clears it" \
    || bad "enter/exit-clear" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 2. Near then far leaves and clears.
p_leave() { printf '0,-60,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 0.3; printf '0,-90,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 0.3; }
run_case p_leave
{ saw ENTER && saw LEAVE && [ "$(off_count)" -ge 1 ]; } \
    && ok "near then far -> LEAVE, permit cleared" \
    || bad "leave on far" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 3. Hysteresis: a between-thresholds sample after entering must NOT leave.
p_hyst() { printf '0,-60,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 0.3; printf '0,-78,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 0.3; }
run_case p_hyst
{ saw ENTER && ! saw LEAVE; } \
    && ok "a between-thresholds sample holds state (no flap)" \
    || bad "hysteresis" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 4. Staleness: enter, then silence past STALE_S -> clears without any far sample.
p_stale() { printf '0,-60,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 2.6; }
run_case p_stale
{ saw ENTER && saw STALE && [ "$(off_count)" -ge 1 ]; } \
    && ok "silence past the stale window clears the permit" \
    || bad "staleness" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 5. Refresh: while present, the permit is re-asserted on the timer (>1 ON).
p_refresh() { printf '0,-60,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 1.4; printf '0,-60,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 0.2; }
run_case p_refresh
{ [ "$(on_count)" -ge 2 ]; } \
    && ok "permit is refreshed on the timer while present ($(on_count) asserts)" \
    || bad "refresh" "on_count=$(on_count) stderr=$(tr '\n' '|' <"${STDERR}")"

# 6. Far while absent: no spurious enter or clear.
p_farfirst() { printf '0,-95,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 0.3; }
run_case p_farfirst
{ ! saw ENTER && ! saw LEAVE; } \
    && ok "a far sample while absent does nothing" \
    || bad "far-first" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 7. Garbage lines (header, blank, non-numeric rssi) are skipped, not read as
#    a strong signal.
p_garbage() { printf 'unix_ms,rssi,id\n'; printf '\n'; printf '0,notanumber,aa,1,1,x,VALID\n'; sleep 0.3; }
run_case p_garbage
{ ! saw ENTER; } \
    && ok "header / blank / non-numeric lines are skipped" \
    || bad "garbage handling" "stderr=$(tr '\n' '|' <"${STDERR}")"

# --- the auth gate (E13) ----------------------------------------------------
#
# These are the cases that were unrepresentable before authenticated presence:
# an imposter's beacon is loud and close and still must not unlock anything.

# 8. A strong INVALID beacon must not assert a permit. This is the imposter
#    standing right next to the Mac.
p_imposter() { printf '0,-40,bb,1,1,0000000000000000,INVALID\n'; sleep 0.5; }
run_case p_imposter
{ ! saw ENTER && [ "$(on_count)" = 0 ]; } \
    && ok "a strong INVALID beacon never asserts a permit" \
    || bad "imposter rejected" "on=$(on_count) stderr=$(tr '\n' '|' <"${STDERR}")"

# 9. NOKEY (this Mac holds no key for that slot) is a refusal, not a pass. An
#    unprovisioned Mac must be unable to unlock, not able to unlock for anyone.
p_nokey() { printf '0,-40,bb,1,9,0000000000000000,NOKEY\n'; sleep 0.5; }
run_case p_nokey
{ ! saw ENTER && [ "$(on_count)" = 0 ]; } \
    && ok "NOKEY is a refusal, not a pass" \
    || bad "nokey" "on=$(on_count) stderr=$(tr '\n' '|' <"${STDERR}")"

# 10. A row with no verdict field at all -- what you get by piping the raw
#     scanner straight in, with no verifier in between. Fails closed, so a
#     mis-assembled pipeline produces no permits rather than every permit.
p_noverdict() { printf '0,-40,bb,1,1,deadbeefdeadbeef\n'; sleep 0.5; }
run_case p_noverdict
{ ! saw ENTER && [ "$(on_count)" = 0 ]; } \
    && ok "a row with no verdict field is ignored (no verifier in the pipe)" \
    || bad "missing verdict" "on=$(on_count) stderr=$(tr '\n' '|' <"${STDERR}")"

# 11. INVALID traffic must not keep a permit alive: after entering on a real
#     beacon, a flood of imposter beacons must still go stale and clear. Without
#     this, an attacker who cannot forge a tag could still hold the door open
#     just by transmitting.
p_flood() {
    printf '0,-60,aa,1,1,deadbeefdeadbeef,VALID\n'
    for _ in 1 2 3 4 5 6; do printf '0,-40,bb,1,1,0000000000000000,INVALID\n'; sleep 0.5; done
}
run_case p_flood
{ saw ENTER && saw STALE && [ "$(off_count)" -ge 1 ]; } \
    && ok "a flood of INVALID beacons cannot hold the permit open" \
    || bad "invalid flood" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 12. The bridge must carry no way to switch the auth gate off. A safeguard with
#     an opt-out defaults to whatever someone forgot to set.
grep -qiE 'REQUIRE_AUTH|SKIP_AUTH|AUTH_OPTIONAL' "${BRIDGE}" \
    && bad "no opt-out for the auth gate" "found a bypass variable" \
    || ok "no opt-out for the auth gate"

# 13. The staleness threshold has to clear the radio's measured noise floor, and
#     must not outlive the beacon's own validity.
#
#     Under the noise floor it fires on ordinary scan gaps: a capture on
#     2026-09-10 saw 9.3s between sightings with the phone motionless 1m away, so
#     the original 8 asked for a password roughly every 2.5 minutes for no
#     reason. That tail is macOS's and cannot be reduced from the phone
#     (docs/validation/2026-09-10-scan-cadence.md).
#
#     The ceiling is 2*WINDOW. A verified beacon is accepted across the current
#     window +/-1, so up to that point this value grants nothing a replayed
#     beacon would not already grant. Past it, it does.
#
#     An earlier version capped this at the plugin's PERMIT_FRESHNESS_S instead,
#     on the theory that the permit aged out first. It does not: while the bridge
#     believes the phone is present it re-touches the permit every REFRESH_S. The
#     wrong ceiling is what pinned the default underneath the noise floor.
#
#     Both bounds are read from the files that own them, so moving either one
#     fails this test rather than silently disarming it.
MEASURED_MAX_GAP_S=9
VERIFIER="${HERE}/presence-verify.swift"
default_stale="$(grep -o 'REPOSE_STALE_S:-[0-9]*' "${BRIDGE}" | head -1 | grep -o '[0-9]*$')"
window="$(grep -o 'let windowSeconds: Int64 = [0-9]*' "${VERIFIER}" 2>/dev/null | grep -o '[0-9]*$')"
if [ -z "${default_stale}" ] || [ -z "${window}" ]; then
    bad "staleness clears the noise floor without outliving the beacon" \
        "could not read stale=${default_stale:-?} window=${window:-?}"
elif [ "${default_stale}" -gt "${MEASURED_MAX_GAP_S}" ] && [ "${default_stale}" -le "$((window * 2))" ]; then
    ok "staleness (${default_stale}s) clears the measured ${MEASURED_MAX_GAP_S}s gap and stays within the beacon's $((window * 2))s validity"
else
    bad "staleness clears the noise floor without outliving the beacon" \
        "stale=${default_stale} must be >${MEASURED_MAX_GAP_S} and <=$((window * 2))"
fi

# 14. Whatever STALE_S is, a bridge that dies must still stop granting entry.
#     That guarantee comes from the plugin ageing the permit out, not from this
#     script, so it must hold without the bridge running at all.
PLUGIN_C="${HERE}/../../../native/macos/minimal-auth-plugin/plugin.c"
freshness="$(grep -o '#define PERMIT_FRESHNESS_S[[:space:]]*[0-9]*' "${PLUGIN_C}" 2>/dev/null \
    | grep -o '[0-9]*$')"
refresh="$(grep -o 'REPOSE_REFRESH_S:-[0-9]*' "${BRIDGE}" | head -1 | grep -o '[0-9]*$')"
if [ -n "${freshness}" ] && [ -n "${refresh}" ] && [ "${refresh}" -lt "${freshness}" ]; then
    ok "a dead bridge fails closed in ${freshness}s (refresh ${refresh}s < freshness)"
else
    bad "a dead bridge fails closed" \
        "refresh=${refresh:-?} must be < plugin freshness=${freshness:-?}"
fi

# --- the status file the Mac app reads --------------------------------------

# 15. Publishing is opt-in. Unset REPOSE_STATUS_FILE must change nothing.
STATUS="${SANDBOX}/status"
rm -f "${STATUS}"
p_enter2() { printf '0,-60,aa,1,1,deadbeefdeadbeef,VALID\n'; sleep 0.3; }
run_case p_enter2
[ ! -e "${STATUS}" ] \
    && ok "no status file is written unless one is asked for" \
    || bad "status opt-in" "a file appeared with REPOSE_STATUS_FILE unset"

# 16. With one, near/away transitions are published, and every line carries a
#     clock. A reader cannot otherwise tell "away" from "the bridge died an hour
#     ago and this file stopped moving" -- and showing the second as the first is
#     how a panel ends up confidently green for a process that has exited.
run_status_case() {
    : > "${ACTIONS}"; rm -f "${STATUS}"
    STDERR="${SANDBOX}/stderr.log"
    "$1" | REPOSE_NEAR_DBM=-72 REPOSE_FAR_DBM=-85 \
        REPOSE_STALE_S=2 REPOSE_REFRESH_S=1 REPOSE_STATUS_FILE="${STATUS}" \
        REPOSE_PERMIT_ON_CMD="printf 'ON\n' >> '${ACTIONS}'" \
        REPOSE_PERMIT_OFF_CMD="printf 'OFF\n' >> '${ACTIONS}'" \
        bash "${BRIDGE}" 2> "${STDERR}"
}

run_status_case p_enter2
state="$(cut -d, -f1 "${STATUS}" 2>/dev/null | tail -1)"
stamp="$(cut -d, -f3 "${STATUS}" 2>/dev/null | tail -1)"
{ [ -n "${stamp}" ] && [ "${stamp}" -gt 0 ] 2>/dev/null; } \
    && ok "the status line carries a timestamp (state=${state})" \
    || bad "status timestamp" "got '$(cat "${STATUS}" 2>/dev/null)'"

# Walking away must stop publishing `near`. The final line is `stopped` rather
# than `away`, and that is the point rather than a rounding error: "your phone
# left" and "the thing that watches for your phone has exited" are different
# facts, and a panel that shows the second as the first tells you the feature is
# working while nothing is running.
p_leave2() { printf '0,-60,aa,1,1,dead,VALID\n'; sleep 0.3; printf '0,-90,aa,1,1,dead,VALID\n'; sleep 0.3; }
run_status_case p_leave2
[ "$(cut -d, -f1 "${STATUS}" 2>/dev/null | tail -1)" != near ] \
    && ok "walking away stops publishing near" \
    || bad "away published" "got '$(cat "${STATUS}" 2>/dev/null)'"

# A clean exit says `stopped`, which must be its own word.
[ "$(cut -d, -f1 "${STATUS}" 2>/dev/null | tail -1)" = stopped ] \
    && ok "a finished bridge publishes stopped, not away" \
    || bad "stopped distinct from away" "got '$(cat "${STATUS}" 2>/dev/null)'"

# 17. An INVALID beacon must never publish near -- the panel would be reporting
#     an imposter as the user's phone.
p_imposter2() { printf '0,-40,bb,1,1,0000000000000000,INVALID\n'; sleep 0.5; }
run_status_case p_imposter2
[ "$(cut -d, -f1 "${STATUS}" 2>/dev/null | tail -1)" != near ] \
    && ok "an INVALID beacon never publishes near" \
    || bad "imposter published near" "got '$(cat "${STATUS}" 2>/dev/null)'"

# 18. The words this script publishes must be words the Mac app understands.
#     Two languages, one file format, and nothing in either compiler checks the
#     other -- so a rename here would silently become a panel that shows
#     "transport unavailable" forever while the bridge is working perfectly.
UNLOCK_RS="${HERE}/../../../src-tauri/src/unlock.rs"
if [ -f "${UNLOCK_RS}" ]; then
    missing=""
    for word in near away stopped; do
        grep -q "\"${word}\"" "${UNLOCK_RS}" || missing="${missing} ${word}"
    done
    [ -z "${missing}" ] \
        && ok "every published state word is one the Mac app parses" \
        || bad "state words agree with the app" "unlock.rs does not mention:${missing}"
    # And the app must not be looking for a word this script never writes.
    for word in $(grep -oE '"(near|away|stopped|starting)"' "${UNLOCK_RS}" | tr -d '"' | sort -u); do
        grep -q "publish ${word}\|publish ${word} " "${BRIDGE}" \
          || grep -q "\"${word}\"" "${BRIDGE}" \
          || bad "the app parses a word the bridge never writes" "${word}"
    done
else
    printf '  SKIP state words agree with the app -- unlock.rs not found\n'
fi

# 19. A killed bridge must close the door on its way out, not leave a live permit
#     behind for the freshness window and a status file still saying `near`.
: > "${ACTIONS}"; rm -f "${STATUS}"
( while :; do printf '0,-60,aa,1,1,dead,VALID\n'; sleep 0.4; done ) \
  | REPOSE_NEAR_DBM=-72 REPOSE_FAR_DBM=-85 REPOSE_STALE_S=45 REPOSE_REFRESH_S=5 \
    REPOSE_STATUS_FILE="${STATUS}" \
    REPOSE_PERMIT_ON_CMD="printf 'ON\n' >> '${ACTIONS}'" \
    REPOSE_PERMIT_OFF_CMD="printf 'OFF\n' >> '${ACTIONS}'" \
    bash "${BRIDGE}" 2> "${SANDBOX}/sig.log" &
sig_pid=$!
sleep 2
kill -TERM "${sig_pid}" 2>/dev/null
sleep 2
{ [ "$(cut -d, -f1 "${STATUS}" 2>/dev/null | tail -1)" = stopped ] \
  && grep -q '^OFF$' "${ACTIONS}"; } \
    && ok "a killed bridge clears the permit and publishes stopped" \
    || bad "kill leaves the door open" \
           "status='$(cat "${STATUS}" 2>/dev/null)' actions='$(tr '\n' ' ' <"${ACTIONS}")'"
kill -9 "${sig_pid}" 2>/dev/null

echo
echo "${pass} passed, ${fail} failed"
[ "${fail}" = 0 ]
