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
on_count()  { grep -c '^ON$'  "${ACTIONS}" 2>/dev/null || echo 0; }
off_count() { grep -c '^OFF$' "${ACTIONS}" 2>/dev/null || echo 0; }

echo "permit-bridge.sh"

# 1. A near sample enters and asserts the permit; EOF then clears it.
p_enter() { printf '0,-60,aa\n'; sleep 0.3; }
run_case p_enter
{ saw ENTER && [ "$(on_count)" -ge 1 ] && [ "$(off_count)" -ge 1 ]; } \
    && ok "near sample enters (asserts permit), stream end clears it" \
    || bad "enter/exit-clear" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 2. Near then far leaves and clears.
p_leave() { printf '0,-60,aa\n'; sleep 0.3; printf '0,-90,aa\n'; sleep 0.3; }
run_case p_leave
{ saw ENTER && saw LEAVE && [ "$(off_count)" -ge 1 ]; } \
    && ok "near then far -> LEAVE, permit cleared" \
    || bad "leave on far" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 3. Hysteresis: a between-thresholds sample after entering must NOT leave.
p_hyst() { printf '0,-60,aa\n'; sleep 0.3; printf '0,-78,aa\n'; sleep 0.3; }
run_case p_hyst
{ saw ENTER && ! saw LEAVE; } \
    && ok "a between-thresholds sample holds state (no flap)" \
    || bad "hysteresis" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 4. Staleness: enter, then silence past STALE_S -> clears without any far sample.
p_stale() { printf '0,-60,aa\n'; sleep 2.6; }
run_case p_stale
{ saw ENTER && saw STALE && [ "$(off_count)" -ge 1 ]; } \
    && ok "silence past the stale window clears the permit" \
    || bad "staleness" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 5. Refresh: while present, the permit is re-asserted on the timer (>1 ON).
p_refresh() { printf '0,-60,aa\n'; sleep 1.4; printf '0,-60,aa\n'; sleep 0.2; }
run_case p_refresh
{ [ "$(on_count)" -ge 2 ]; } \
    && ok "permit is refreshed on the timer while present ($(on_count) asserts)" \
    || bad "refresh" "on_count=$(on_count) stderr=$(tr '\n' '|' <"${STDERR}")"

# 6. Far while absent: no spurious enter or clear.
p_farfirst() { printf '0,-95,aa\n'; sleep 0.3; }
run_case p_farfirst
{ ! saw ENTER && ! saw LEAVE; } \
    && ok "a far sample while absent does nothing" \
    || bad "far-first" "stderr=$(tr '\n' '|' <"${STDERR}")"

# 7. Garbage lines (header, blank, non-numeric rssi) are skipped, not read as
#    a strong signal.
p_garbage() { printf 'unix_ms,rssi,id\n'; printf '\n'; printf '0,notanumber,aa\n'; sleep 0.3; }
run_case p_garbage
{ ! saw ENTER; } \
    && ok "header / blank / non-numeric lines are skipped" \
    || bad "garbage handling" "stderr=$(tr '\n' '|' <"${STDERR}")"

echo
echo "${pass} passed, ${fail} failed"
[ "${fail}" = 0 ]
