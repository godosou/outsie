#!/bin/bash
#
# install-permitd.sh -- install and start repose-permitd as a root LaunchDaemon.
#
# This is a NEW, standalone installer; it does NOT modify the plugin's
# install.sh. It is meant to run BEFORE (or alongside) the plugin install: the
# daemon must be listening before the permit mechanism starts asking it, or
# every unlock attempt fails closed to the password (which is safe, just not the
# feature).
#
# Throwaway spike. Run only on a disposable, rollback-capable test machine.
#
set -euo pipefail

LABEL="ai.repose.spike.permitd"
SUPPORT_DIR="/Library/Application Support/ReposeSpike"
DEST_BIN="${SUPPORT_DIR}/repose-permitd"
DAEMON_PLIST="/Library/LaunchDaemons/${LABEL}.plist"

cd "$(dirname "$0")"
[[ $EUID -eq 0 ]] || { echo "Must run as root (sudo $0)." >&2; exit 1; }
[[ -x build/repose-permitd ]] || { echo "Missing build/repose-permitd. Run ./build.sh first." >&2; exit 1; }

echo "==> Installing daemon binary to ${DEST_BIN}"
install -d -o root -g wheel -m 0755 "${SUPPORT_DIR}"
# Stage beside the destination and swap, so a failed copy never leaves a daemon
# plist pointing at a missing binary.
install -o root -g wheel -m 0755 build/repose-permitd "${DEST_BIN}.incoming"
mv "${DEST_BIN}.incoming" "${DEST_BIN}"

echo "==> Installing LaunchDaemon ${DAEMON_PLIST}"
install -o root -g wheel -m 0644 "${LABEL}.plist" "${DAEMON_PLIST}"

echo "==> (Re)loading ${LABEL}"
launchctl bootout "system/${LABEL}" 2>/dev/null || true
launchctl bootstrap system "${DAEMON_PLIST}" \
    || { echo "bootstrap failed" >&2; exit 1; }
launchctl kickstart -k "system/${LABEL}" 2>/dev/null || true

SOCK="/var/run/repose-permitd/permit.sock"
echo "==> Waiting for the socket to appear"
for _ in $(seq 1 20); do
    [[ -S "${SOCK}" ]] && break
    sleep 0.2
done

if [[ -S "${SOCK}" ]]; then
    echo "Done. Socket and directory:"
    ls -ld /var/run/repose-permitd
    ls -l  "${SOCK}"
    echo
    echo "Expected: dir  drwxr-x---  root  _securityagent   (0750 root:92)"
    echo "          sock srw-rw----  root  _securityagent   (0660 root:92)"
    echo
    echo "Logs: /var/log/repose-permitd.log"
else
    echo "Socket did not appear; check /var/log/repose-permitd.log" >&2
    exit 1
fi

cat <<'EOF'

Uninstall:
  sudo launchctl bootout system/ai.repose.spike.permitd
  sudo rm -f "/Library/LaunchDaemons/ai.repose.spike.permitd.plist"
  sudo rm -f "/Library/Application Support/ReposeSpike/repose-permitd"
  sudo rm -rf /var/run/repose-permitd
EOF
