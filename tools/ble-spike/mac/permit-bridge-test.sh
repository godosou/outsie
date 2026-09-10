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

# 13. The staleness threshold has to sit between the radio's measured noise floor
#     and the plugin's own freshness bound.
#
#     Below the noise floor it fires on ordinary scan gaps -- a 5-minute capture
#     on 2026-09-10 saw a 8.98s gap with the phone motionless 1m away, so at the
#     old default of 8 the screen demanded a password roughly every 2.5 minutes
#     for no reason. At or above the plugin's PERMIT_FRESHNESS_S it stops adding
#     anything, because the permit has already aged out by itself.
#
#     The upper bound is read out of plugin.c rather than written here, so
#     changing it there fails this test instead of silently disarming the check.
MEASURED_MAX_GAP_S=9
PLUGIN_C="${HERE}/../../../native/macos/minimal-auth-plugin/plugin.c"
default_stale="$(grep -o 'REPOSE_STALE_S:-[0-9]*' "${BRIDGE}" | head -1 | grep -o '[0-9]*$')"
freshness="$(grep -o '#define PERMIT_FRESHNESS_S[[:space:]]*[0-9]*' "${PLUGIN_C}" 2>/dev/null \
    | grep -o '[0-9]*$')"
if [ -z "${default_stale}" ] || [ -z "${freshness}" ]; then
    bad "staleness sits between the noise floor and the permit's freshness" \
        "could not read stale=${default_stale:-?} freshness=${freshness:-?}"
elif [ "${default_stale}" -gt "${MEASURED_MAX_GAP_S}" ] && [ "${default_stale}" -lt "${freshness}" ]; then
    ok "staleness (${default_stale}s) is above the measured ${MEASURED_MAX_GAP_S}s gap and below the permit's ${freshness}s freshness"
else
    bad "staleness sits between the noise floor and the permit's freshness" \
        "stale=${default_stale} must be >${MEASURED_MAX_GAP_S} and <${freshness}"
fi

echo
echo "${pass} passed, ${fail} failed"
[ "${fail}" = 0 ]
