#!/bin/bash
# THE acceptance test for phone proximity unlock.
#
# This is the only test that measures progress on this feature. Unit tests going
# green is not progress; this going green is.
#
# The test states the product promise directly:
#
#   screen is locked -> phone leaves  -> Mac stays locked
#                    -> phone returns -> waking the Mac unlocks it, no password
#
# Waking is part of the promise, not a shortcut. macOS only evaluates the
# screensaver authorization when an unlock is actually attempted, which is also
# how Apple Watch unlock behaves: the phone removes the typing, not the waking.
#
# The phone's actions are injected, so the same assertions survive every phase
# without being rewritten:
#
#   A4 (simulated presence, real plugin, no BLE, no crypto):
#     REPOSE_LEAVE_CMD='ssh vm rm -f /tmp/repose-permit'
#     REPOSE_RETURN_CMD='ssh vm touch /tmp/repose-permit'
#   B2 (real BLE, no crypto): the permit bridge drives the same two commands.
#   B4 (durability): a human walks away and back.
#
# SAFETY: this script locks the screen. It refuses to do so unless
# REPOSE_E2E_ALLOW_LOCK=1 is set. Run it on the throwaway VM, not on the Mac you
# need to keep working on. Use --dry-run to validate the wiring without locking
# anything.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOCKSTATE="${HERE}/lockstate.sh"

DEADLINE_MS="${REPOSE_UNLOCK_DEADLINE_MS:-3000}"
LOCK_WAIT_S="${REPOSE_LOCK_WAIT_S:-20}"
STAY_LOCKED_S="${REPOSE_STAY_LOCKED_S:-5}"
LEAVE_CMD="${REPOSE_LEAVE_CMD:-}"
RETURN_CMD="${REPOSE_RETURN_CMD:-}"

# Locking is the one action that must happen on the machine under test rather
# than the machine running the test. `open -a ScreenSaverEngine` only works from
# inside the target's GUI session, so driving a VM over ssh needs
#   launchctl asuser $(id -u admin) open -a ScreenSaverEngine
# with `pmset displaysleepnow` as the fallback if that is refused.
LOCK_CMD="${REPOSE_LOCK_CMD:-open -a ScreenSaverEngine}"
TARGET="${REPOSE_TARGET:-this machine}"

# macOS does not evaluate system.login.screensaver while the machine sits
# locked and idle. The evaluation -- and therefore MechanismInvoke -- starts
# when the user wakes the machine and an unlock is actually attempted. A test
# that never wakes the target would find the mechanism had never run, time out,
# and read exactly like a plugin macOS refused to load.
#
# This is also how the feature behaves in practice, and how Apple Watch unlock
# behaves: you wake the Mac, and then it unlocks without a password. The phone
# removes the typing, not the waking.
WAKE_CMD="${REPOSE_WAKE_CMD:-caffeinate -u -t 1}"

# Optional precondition check. A screen-lock delay other than "immediate" means
# locking only dims the display and the session is never actually locked, so
# every assertion below would be about a machine that was never locked. The
# failure is silent in the worst way: with a plugin installed you get a log line
# proving the mechanism ran next to an oracle reading unlocked.
LOCK_PRECHECK_CMD="${REPOSE_LOCK_PRECHECK_CMD:-}"

DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

now_ms() { perl -MTime::HiRes=time -e 'printf "%.0f\n", time()*1000'; }
say() { printf '%s\n' "$*"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

lock_state() { "$LOCKSTATE"; }

# Three states, never two. lockstate.sh exits 0 for locked, 1 for unlocked and 2
# when it could not read at all. Collapsing 2 into "not locked" is how a dropped
# ssh connection turns into a reported unlock: the final step of this test waits
# for "not locked", so one unreadable poll at the wrong moment would print PASS
# with nothing having been unlocked.
is_locked() {
  "$LOCKSTATE" --is-locked
  return $?
}

# Poll until `is_locked` matches $1 ("true"/"false"), or $2 milliseconds pass.
# Prints the elapsed milliseconds.
#   0 = reached the wanted state
#   1 = timed out
#   3 = the oracle stopped being readable; the run is void, not failed
#
# 3 is a separate code because this runs inside a command substitution, where a
# plain exit would only leave the subshell and read back as an ordinary timeout.
wait_for_lock_state() {
  local want="$1" budget_ms="$2" start now rc
  start="$(now_ms)"
  while :; do
    is_locked
    rc=$?
    case "$rc" in
      0) [ "$want" = "true" ] && break ;;
      1) [ "$want" = "false" ] && break ;;
      *)
        now="$(now_ms)"
        printf '%s' "$((now - start))"
        return 3
        ;;
    esac
    now="$(now_ms)"
    if [ $((now - start)) -ge "$budget_ms" ]; then
      printf '%s' "$((now - start))"
      return 1
    fi
    sleep 0.1
  done
  now="$(now_ms)"
  printf '%s' "$((now - start))"
}

# Wrapper that turns an unreadable oracle into an immediate abort at the call
# site, where exiting actually works.
oracle_died() {
  fail "the lock-state oracle stopped being readable ${1}ms into this step.
      The result is void, not a pass or a fail: an unreadable target must never
      be reported as unlocked. Check the connection to ${TARGET} and rerun."
}

run_hook() {
  local name="$1" cmd="$2"
  say "  -> ${name}: ${cmd}"
  if ! bash -c "$cmd"; then
    fail "${name} hook exited non-zero: ${cmd}"
  fi
}

preflight() {
  [ -x "$LOCKSTATE" ] || fail "missing oracle: ${LOCKSTATE}"
  "$LOCKSTATE" --raw >/dev/null 2>&1 || fail "lock-state oracle unreadable on this host"
  [ -n "$LEAVE_CMD" ] || fail "REPOSE_LEAVE_CMD is not set (see header for per-step values)"
  [ -n "$RETURN_CMD" ] || fail "REPOSE_RETURN_CMD is not set (see header for per-step values)"
  if [ -n "$LOCK_PRECHECK_CMD" ]; then
    local delay
    delay="$(bash -c "$LOCK_PRECHECK_CMD" 2>&1 | tail -1)"
    case "$delay" in
      *immediate*) say "  screen lock   : immediate" ;;
      *) fail "the target's screen-lock delay is not immediate:
      ${delay}
      Locking would only dim the display and leave the session unlocked, so
      nothing this test measures would be real. Fix it on the target:
        sysadminctl -screenLock immediate -password <pw>" ;;
    esac
  fi

  # Only the real run needs to begin unlocked. A dry run never locks anything,
  # so refusing there would make the wiring check fail on a machine that simply
  # happens to be locked -- which is most machines nobody is sitting at.
  if [ "$DRY_RUN" != "1" ]; then
    is_locked; local pre_rc=$?
    [ "$pre_rc" -le 1 ] || fail "the lock-state oracle is unreadable (exit ${pre_rc});
      refusing to start rather than guessing the target's state"
    [ "$pre_rc" != "0" ] || fail "screen is already locked; this test must start from an unlocked session"
  fi
}

main() {
  say "outsie phone-unlock acceptance test"
  say "  target        : ${TARGET}"
  say "  driven from   : $(hostname -s) / macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  say "  lock via      : ${LOCK_CMD}"
  say "  wake via      : ${WAKE_CMD}"
  say "  unlock budget : ${DEADLINE_MS} ms"
  say "  start state   : $(lock_state)"
  say ""

  preflight

  if [ "$DRY_RUN" = "1" ]; then
    say "DRY RUN: exercising hooks and oracle only, never locking the screen."
    run_hook "leave " "$LEAVE_CMD"
    say "     state after leave : $(lock_state)"
    run_hook "return" "$RETURN_CMD"
    say "     state after return: $(lock_state)"
    run_hook "wake  " "$WAKE_CMD"
    say ""
    say "DRY RUN OK: oracle readable, both hooks executable. Wiring is sound."
    say "Set REPOSE_E2E_ALLOW_LOCK=1 and drop --dry-run to run the real test."
    exit 0
  fi

  if [ "${REPOSE_E2E_ALLOW_LOCK:-}" != "1" ]; then
    fail "refusing to lock the screen. Set REPOSE_E2E_ALLOW_LOCK=1 to arm, or pass --dry-run."
  fi

  say "Locking the screen in 5 seconds (ctrl-c to abort)..."
  sleep 5

  # 1. Lock, and confirm the system actually reached the locked state.
  run_hook "lock  " "$LOCK_CMD"
  local elapsed rc
  elapsed="$(wait_for_lock_state true $((LOCK_WAIT_S * 1000)))"; rc=$?
  [ "$rc" = "3" ] && oracle_died "$elapsed"
  if [ "$rc" != "0" ]; then
    fail "screen did not lock within ${LOCK_WAIT_S}s (elapsed ${elapsed}ms).
      Check: System Settings > Lock Screen > 'Require password after screen saver
      begins' must be 'Immediately'. Without that there is nothing to unlock."
  fi
  say "[1/3] locked after ${elapsed}ms"

  # 2. Phone leaves, then somebody tries to get in anyway. The Mac must NOT
  #    unlock. Waking here is what makes the assertion mean anything: without an
  #    unlock attempt macOS never evaluates the rule, the mechanism never runs,
  #    and "stayed locked" would be true of a machine with no plugin at all.
  run_hook "leave " "$LEAVE_CMD"
  run_hook "wake  " "$WAKE_CMD"
  elapsed="$(wait_for_lock_state false $((STAY_LOCKED_S * 1000)))"; rc=$?
  [ "$rc" = "3" ] && oracle_died "$elapsed"
  if [ "$rc" = "0" ]; then
    fail "Mac unlocked ${elapsed}ms after the phone LEFT. Unlock is not gated on presence."
  fi
  say "[2/3] stayed locked through a wake attempt while the phone was away"

  # 3. Phone returns. This is the promise.
  local t0 t1
  t0="$(now_ms)"
  run_hook "return" "$RETURN_CMD"
  # Waking is what starts the authorization evaluation. Without it the
  # mechanism is never invoked and the wait below would time out against a
  # plugin that is installed and working perfectly.
  run_hook "wake  " "$WAKE_CMD"
  elapsed="$(wait_for_lock_state false "$DEADLINE_MS")"; rc=$?
  [ "$rc" = "3" ] && oracle_died "$elapsed"
  if [ "$rc" != "0" ]; then
    say ""
    fail "Mac did NOT unlock within ${DEADLINE_MS}ms of the phone returning (still locked).
      This is the expected failure until Step 2 lands a loadable Authorization
      Plugin. Password entry is unaffected."
  fi
  t1="$(now_ms)"
  say "[3/3] unlocked ${elapsed}ms after the phone returned"
  say ""
  say "PASS  end-to-end unlock latency: $((t1 - t0)) ms (budget ${DEADLINE_MS} ms)"
}

# Sourcing the script exposes the helpers without running the test, so the
# polling/timeout logic can be verified on its own (see harness_selftest.sh).
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
