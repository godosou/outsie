#!/bin/bash
# Self-test for the acceptance harness.
#
# The acceptance test cannot run without locking a screen, so the logic it
# depends on -- polling for a lock-state transition, and giving up on deadline
# -- is verified here against a fake oracle instead. Runs anywhere, locks
# nothing, takes about a second.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PASS=0
FAIL=0

check() {
  local name="$1" want="$2" got="$3"
  if [ "$want" = "$got" ]; then
    printf '  ok   %s\n' "$name"
    PASS=$((PASS + 1))
  else
    printf '  FAIL %s (want %q, got %q)\n' "$name" "$want" "$got"
    FAIL=$((FAIL + 1))
  fi
}

# Load the helpers without running the test.
# shellcheck source=/dev/null
source "${HERE}/unlock_acceptance.sh"

# Fake oracle: `is_locked` reflects the contents of a scratch file, so a test
# can flip the "screen" at a chosen moment.
STATE_FILE="$(mktemp -t repose-lockstate)"
trap 'rm -f "$STATE_FILE"' EXIT
is_locked() { [ "$(cat "$STATE_FILE")" = "true" ]; }

echo "harness self-test"

# 1. Already in the wanted state -> returns immediately, success.
echo "true" > "$STATE_FILE"
elapsed="$(wait_for_lock_state true 2000)"
check "returns at once when already locked" "0" "$?"
[ "$elapsed" -lt 500 ] \
  && check "immediate return is fast" "fast" "fast" \
  || check "immediate return is fast" "fast" "slow (${elapsed}ms)"

# 2. Never reaches the wanted state -> times out, non-zero, honours the budget.
echo "false" > "$STATE_FILE"
elapsed="$(wait_for_lock_state true 700)"; rc=$?
check "times out when state never arrives" "1" "$rc"
[ "$elapsed" -ge 700 ] && [ "$elapsed" -lt 2000 ] \
  && check "timeout honours the budget" "in-range" "in-range" \
  || check "timeout honours the budget" "in-range" "${elapsed}ms"

# 3. State flips mid-wait -> observed, success, before the deadline.
echo "false" > "$STATE_FILE"
( sleep 0.6; echo "true" > "$STATE_FILE" ) &
elapsed="$(wait_for_lock_state true 5000)"; rc=$?
wait
check "observes a mid-wait transition" "0" "$rc"
[ "$elapsed" -ge 500 ] && [ "$elapsed" -lt 2500 ] \
  && check "transition timing is plausible" "in-range" "in-range" \
  || check "transition timing is plausible" "in-range" "${elapsed}ms"

# 4. Waiting for the opposite state works the same way (used by the
#    "must stay locked while the phone is away" assertion).
echo "true" > "$STATE_FILE"
elapsed="$(wait_for_lock_state false 700)"; rc=$?
check "times out waiting for unlock while locked" "1" "$rc"

# 5. The remote injection point. The whole VM-driven skeleton rests on
#    REPOSE_LOCKSTATE_CMD: if it silently returned the host's state instead of
#    the target's, every result would be about the wrong machine.
LS="${HERE}/lockstate.sh"

run_ls() { env REPOSE_LOCKSTATE_CMD="$1" "$LS" ${2:-}; }

check "injected command is used instead of local ioreg" \
  "locked" "$(run_ls 'echo true' 2>/dev/null)"
check "injected 'false' reads as unlocked" \
  "unlocked" "$(run_ls 'echo false' 2>/dev/null)"
check "surrounding whitespace is tolerated" \
  "unlocked" "$(run_ls 'printf "  false \n"' 2>/dev/null)"
check "--raw passes the injected value through" \
  "true" "$(run_ls 'echo true' --raw 2>/dev/null)"

run_ls 'echo true' --is-locked >/dev/null 2>&1
check "--is-locked exits 0 for an injected locked state" "0" "$?"
run_ls 'echo false' --is-locked >/dev/null 2>&1
check "--is-locked exits 1 for an injected unlocked state" "1" "$?"

# A remote read that fails must not be reported as "unlocked". Treating an
# unreachable target as unlocked would make the acceptance test pass without
# anything having been unlocked at all.
run_ls 'exit 7' >/dev/null 2>&1
check "a failing injected command exits 2, not 0" "2" "$?"
run_ls 'echo neither-true-nor-false' >/dev/null 2>&1
check "an unparseable injected value exits 2" "2" "$?"
run_ls 'echo ""' >/dev/null 2>&1
check "an empty injected value exits 2" "2" "$?"

# With nothing injected the oracle must still read this machine.
local_state="$(env -u REPOSE_LOCKSTATE_CMD "$LS" --raw 2>/dev/null)"
[ "$local_state" = "true" ] || [ "$local_state" = "false" ] \
  && check "local reading still works when nothing is injected" "yes" "yes" \
  || check "local reading still works when nothing is injected" "yes" "got '$local_state'"

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
