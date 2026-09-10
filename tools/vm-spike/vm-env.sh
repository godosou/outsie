#!/bin/bash
# Source this to point the acceptance harness at the throwaway VM.
#
#   source tools/vm-spike/vm-env.sh
#   tests/e2e/unlock_acceptance.sh --dry-run
#   REPOSE_E2E_ALLOW_LOCK=1 tests/e2e/unlock_acceptance.sh
#
# The harness runs on the host; the machine that gets locked and unlocked is the
# VM. Every host-vs-target difference is expressed through the injection points
# the harness already has, so nothing about the test logic changes.
#
# NOTE: not yet exercised against a real VM -- the image is still downloading.
# The first run is expected to need adjustment, most likely in REPOSE_LOCK_CMD.

VM="${REPOSE_VM:-repose-spike}"
VM_USER="${REPOSE_VM_USER:-admin}"

if ! command -v tart >/dev/null 2>&1; then
  echo "vm-env: tart is not installed" >&2
  return 1 2>/dev/null || exit 1
fi

VM_IP="$(tart ip "$VM" 2>/dev/null)"
if [ -z "$VM_IP" ]; then
  echo "vm-env: '${VM}' is not running. Start it first:  tart run ${VM}" >&2
  return 1 2>/dev/null || exit 1
fi

# The lock-state oracle is polled every 100ms and the unlock budget is 3s. A
# fresh ssh handshake costs 200-500ms, which would both starve the sampling and
# charge ssh's latency to the feature being measured. One multiplexed connection
# makes each poll a few milliseconds.
# Exported, not just set: run-a1.sh is a child process and cannot see a plain
# shell variable. Without this it aborts saying vm-env.sh was never sourced,
# which is a confusing way to report a missing export.
export REPOSE_SSH_SOCKET="/tmp/repose-vm-${VM}.sock"
export REPOSE_SSH="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
-o LogLevel=ERROR -o ControlMaster=auto -o ControlPath=${REPOSE_SSH_SOCKET} \
-o ControlPersist=10m ${VM_USER}@${VM_IP}"

export REPOSE_TARGET="${VM} (${VM_USER}@${VM_IP})"

# Reading IOConsoleLocked needs no GUI session and no TCC grant, so it is happy
# over ssh.
export REPOSE_LOCKSTATE_CMD="${REPOSE_SSH} 'ioreg -n Root -d1 -a | plutil -extract IOConsoleLocked raw -o - -'"

# Locking has to produce a genuinely locked session, not a dark screen.
#
# An earlier version used `pmset displaysleepnow` because it needs no root. That
# was wrong in a way that would have wasted a whole experiment: displaysleepnow
# only sleeps the display. Whether the session locks depends entirely on the
# screen-lock delay. With a non-zero delay the session is not locked at all
# during the grace period, the oracle reads unlocked, and with the plugin
# installed you get the worst possible observation -- a plugin log line proving
# the mechanism ran, alongside IOConsoleLocked=false. A contradiction like that
# is exactly how this project talks itself into a confident wrong answer.
#
# So: assert the precondition, then drive the real screensaver inside the Aqua
# session. `sysadminctl -screenLock status` reads the delay without root or a
# password (verified on 14.6.1), which turns "remember to set it" into something
# the harness can check.
export REPOSE_LOCK_PRECHECK_CMD="${REPOSE_SSH} 'sysadminctl -screenLock status 2>&1'"
export REPOSE_LOCK_CMD="${REPOSE_SSH} 'sudo launchctl asuser \$(stat -f %u /dev/console) open -a /System/Library/CoreServices/ScreenSaverEngine.app'"

# Waking is a GUI action, not an ssh one. A1 measured that the mechanism is not
# invoked when the machine locks, nor when the password field appears -- only
# when an unlock is submitted. caffeinate -u, which an earlier version used,
# asserts user activity without submitting anything, so the mechanism never ran
# and the result was indistinguishable from a plugin macOS had refused to load.
# Resolved from the repo root rather than BASH_SOURCE: this file is sourced,
# and under zsh BASH_SOURCE does not name it, so the path silently came out
# wrong -- pointing at a script that does not exist, which would have looked
# like a wake that did nothing.
export REPOSE_WAKE_CMD="$(git -C "$(pwd)" rev-parse --show-toplevel 2>/dev/null)/tools/vm-spike/vm-wake-submit.sh"

# The simulated presence source for the first phase. Phase two replaces these
# two commands with the real BLE bridge and changes nothing else.
#
# The permit is now hardened: it must be root-owned and fresh in the root-only
# directory /var/run/repose-spike. So the "phone returned" command writes it as
# root (sudo) -- exactly what the real BLE bridge will do. A non-root or stale
# permit is ignored by the plugin, which is the security property under test.
export REPOSE_LEAVE_CMD="${REPOSE_SSH} 'sudo rm -f /var/run/repose-spike/permit'"
export REPOSE_RETURN_CMD="${REPOSE_SSH} 'sudo mkdir -p /var/run/repose-spike && sudo chmod 755 /var/run/repose-spike && sudo touch /var/run/repose-spike/permit'"

repose_vm_check() {
  echo "target      ${REPOSE_TARGET}"
  printf 'ssh         '
  if eval "${REPOSE_SSH} 'echo reachable'" 2>&1; then :; else
    echo "UNREACHABLE (password for a hand-made account is whatever you set)"
    return 1
  fi
  printf 'lock state  '
  eval "${REPOSE_LOCKSTATE_CMD}" 2>&1 || echo "oracle unreadable"
  printf 'screen lock '
  local delay
  delay="$(eval "${REPOSE_LOCK_PRECHECK_CMD}" 2>&1 | tail -1)"
  echo "${delay}"
  case "${delay}" in
    *immediate*) ;;
    *) echo "  ^^ NOT immediate. Locking will only dim the screen and leave the"
       echo "     session unlocked during the grace period. Fix it first:"
       echo "     ssh in and run: sysadminctl -screenLock immediate -password <pw>" ;;
  esac
  printf 'passwordless sudo '
  eval "${REPOSE_SSH} 'sudo -n true'" >/dev/null 2>&1 \
    && echo "yes" \
    || echo "NO -- REPOSE_LOCK_CMD needs it; see FIRST-BOOT.md"
  printf 'gui session '
  if repose_vm_gui_ready >/dev/null 2>&1; then
    echo "logged in (console uid $(eval "${REPOSE_CONSOLE_UID_CMD}" 2>/dev/null | tr -d '[:space:]'))"
  else
    echo "NOBODY LOGGED IN -- that screen is the boot login window, which does not"
    echo "               use the screensaver right; the mechanism will never run there"
  fi
  printf 'guest os    '
  eval "${REPOSE_SSH} 'sw_vers -productVersion; sw_vers -buildVersion'" 2>&1 | tr '\n' ' '
  echo
}

# Is anyone actually logged into the guest's GUI?
#
# This cost several rounds of wrong diagnosis. A freshly booted VM sits at the
# BOOT LOGIN WINDOW, which evaluates `system.login.console`. Our mechanism is
# installed on `system.login.screensaver`, so it is never consulted there -- the
# plugin log stays empty, the screen stays locked, and every symptom reads as
# "macOS refused to load the plugin". Nothing was wrong with the plugin; the test
# was knocking on a different door.
#
# The two screens look nearly identical, which is what makes it expensive. This
# tells them apart: with nobody logged in, /dev/console belongs to root.
export REPOSE_CONSOLE_UID_CMD="${REPOSE_SSH} 'stat -f %u /dev/console'"

repose_vm_gui_ready() {
  local uid
  uid="$(eval "${REPOSE_CONSOLE_UID_CMD}" 2>/dev/null | tr -d '[:space:]')"
  if [ "${uid}" = "0" ] || [ -z "${uid}" ]; then
    echo "vm-env: nobody is logged into ${VM}'s GUI (console uid=${uid:-unreadable})." >&2
    echo "  That screen is the BOOT LOGIN WINDOW, which uses system.login.console --" >&2
    echo "  a different right from the system.login.screensaver our mechanism is on." >&2
    echo "  Testing there measures nothing: log in first, then lock the screen." >&2
    return 1
  fi
  return 0
}

repose_vm_close() {
  ssh -o ControlPath="${REPOSE_SSH_SOCKET}" -O exit "${VM_USER}@${VM_IP}" 2>/dev/null
  echo "closed the multiplexed connection"
}

echo "vm-env: driving ${REPOSE_TARGET}"
echo "  run 'repose_vm_check' to verify reachability before locking anything"
echo "  run 'repose_vm_close' when finished"
