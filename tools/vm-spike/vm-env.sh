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
REPOSE_SSH_SOCKET="/tmp/repose-vm-${VM}.sock"
REPOSE_SSH="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
-o LogLevel=ERROR -o ControlMaster=auto -o ControlPath=${REPOSE_SSH_SOCKET} \
-o ControlPersist=10m ${VM_USER}@${VM_IP}"

export REPOSE_TARGET="${VM} (${VM_USER}@${VM_IP})"

# Reading IOConsoleLocked needs no GUI session and no TCC grant, so it is happy
# over ssh.
export REPOSE_LOCKSTATE_CMD="${REPOSE_SSH} 'ioreg -n Root -d1 -a | plutil -extract IOConsoleLocked raw -o - -'"

# `open -a ScreenSaverEngine` only works from inside the target's Aqua session,
# which an ssh session is not part of. `launchctl asuser` would bridge that but
# needs root, and passwordless sudo is not guaranteed on a VM whose account was
# created by hand. `pmset displaysleepnow` needs neither: waking the display
# with "require password immediately" set goes through the same
# system.login.screensaver right the plugin hooks.
export REPOSE_LOCK_CMD="${REPOSE_SSH} 'pmset displaysleepnow'"

# The simulated presence source for the first phase. Phase two replaces these
# two commands with the real BLE bridge and changes nothing else.
export REPOSE_LEAVE_CMD="${REPOSE_SSH} 'rm -f /tmp/repose-permit'"
export REPOSE_RETURN_CMD="${REPOSE_SSH} 'touch /tmp/repose-permit'"

repose_vm_check() {
  echo "target      ${REPOSE_TARGET}"
  printf 'ssh         '
  if eval "${REPOSE_SSH} 'echo reachable'" 2>&1; then :; else
    echo "UNREACHABLE (password for a hand-made account is whatever you set)"
    return 1
  fi
  printf 'lock state  '
  eval "${REPOSE_LOCKSTATE_CMD}" 2>&1 || echo "oracle unreadable"
  printf 'guest os    '
  eval "${REPOSE_SSH} 'sw_vers -productVersion; sw_vers -buildVersion'" 2>&1 | tr '\n' ' '
  echo
}

repose_vm_close() {
  ssh -o ControlPath="${REPOSE_SSH_SOCKET}" -O exit "${VM_USER}@${VM_IP}" 2>/dev/null
  echo "closed the multiplexed connection"
}

echo "vm-env: driving ${REPOSE_TARGET}"
echo "  run 'repose_vm_check' to verify reachability before locking anything"
echo "  run 'repose_vm_close' when finished"
