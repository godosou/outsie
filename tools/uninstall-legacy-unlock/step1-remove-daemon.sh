#!/bin/bash
# Step 1 of 2: remove the ai.repose.unlockd root daemon.
#
# WHY THIS EXISTS
# ---------------
# On 2026-09-08 15:23 an automated run of `repose-unlockctl install --apply`
# installed a live root daemon on this Mac, even though the branch's own
# docs/validation/verification-summary.md states that no Authorization Plugin or
# launchd service was ever installed and that no --apply command was ever run.
# The daemon's control socket was created world-writable:
#
#   srw-rw-rw-  root  daemon  /private/var/run/ai.repose.unlock-control.sock
#
# (SockPathMode 438 = 0666, versus 0600 on the sibling consume socket.) A root
# daemon with a world-writable control entry point, RunAtLoad and KeepAlive does
# not belong on a daily-driver machine.
#
# WHAT THIS SCRIPT DOES NOT DO
# ----------------------------
# It does not touch the authorization database. Removing a launchd job cannot
# affect your ability to unlock the screen. The screensaver rule still ends in
# `use-login-window-ui`, so the password path stays intact either way.
# The authorizationdb cleanup is step 2, deliberately separate.
#
# Usage:  sudo bash step1-remove-daemon.sh          (prompts before changing anything)
#         sudo bash step1-remove-daemon.sh --yes    (no prompt)

set -uo pipefail

LABEL="ai.repose.unlockd"
PLIST="/Library/LaunchDaemons/${LABEL}.plist"
BINARY="/Library/PrivilegedHelperTools/${LABEL}"
CONTROL_SOCK="/private/var/run/ai.repose.unlock-control.sock"
SOCK_DIR="/var/run/${LABEL}"

if [ "$(id -u)" -ne 0 ]; then
  echo "This script must run as root: sudo bash $0" >&2
  exit 1
fi

show_state() {
  echo "  daemon process : $(pgrep -f "${LABEL}" | tr '\n' ' ' | sed 's/ $//' || true)"
  echo "  launchd job    : $(launchctl print "system/${LABEL}" >/dev/null 2>&1 && echo loaded || echo "not loaded")"
  for p in "$PLIST" "$BINARY" "$CONTROL_SOCK" "$SOCK_DIR"; do
    if [ -e "$p" ]; then printf '  %-14s %s\n' "present" "$(ls -lad "$p")"; else printf '  %-14s %s\n' "absent" "$p"; fi
  done
}

echo "=== current state ==="
show_state
echo

echo "=== will run ==="
cat <<EOF
  launchctl bootout system/${LABEL}
  rm -f  ${PLIST}
  rm -f  ${BINARY}
  rm -f  ${CONTROL_SOCK}
  rm -rf ${SOCK_DIR}
EOF
echo
echo "The authorization database is NOT touched by this script."
echo

if [ "${1:-}" != "--yes" ]; then
  printf 'Proceed? [y/N] '
  read -r reply
  case "$reply" in
    y | Y) ;;
    *) echo "Aborted. Nothing was changed."; exit 0 ;;
  esac
fi

echo
echo "-> launchctl bootout system/${LABEL}"
if launchctl bootout "system/${LABEL}" 2>&1; then
  echo "   unloaded"
else
  # bootout returns non-zero when the job is already gone; fall back to the
  # legacy verb for older loading paths, then keep going either way.
  echo "   bootout reported no such job; trying legacy unload"
  launchctl unload "$PLIST" 2>&1 || true
fi

for p in "$PLIST" "$BINARY" "$CONTROL_SOCK"; do
  echo "-> rm -f ${p}"
  rm -f "$p"
done
echo "-> rm -rf ${SOCK_DIR}"
rm -rf "$SOCK_DIR"

echo
echo "=== state after removal ==="
show_state

echo
remaining="$(pgrep -f "${LABEL}" | tr '\n' ' ' | sed 's/ $//')"
if [ -n "$remaining" ]; then
  echo "WARNING: a process matching ${LABEL} is still running: ${remaining}"
  echo "KeepAlive should no longer restart it now that the plist is gone."
  echo "If it persists, reboot and re-run this script."
  exit 1
fi
if [ -e "$PLIST" ] || [ -e "$BINARY" ] || [ -e "$CONTROL_SOCK" ]; then
  echo "WARNING: some paths could not be removed. Review the listing above."
  exit 1
fi

echo "Step 1 complete: the root daemon, its plist, binary and sockets are gone."
echo
echo "Next: confirm the machine still locks and unlocks normally with your"
echo "password, then run step2-remove-authdb.sh to clean the authorization"
echo "database entries (that step needs a terminal window kept open)."
