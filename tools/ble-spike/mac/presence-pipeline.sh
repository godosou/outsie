#!/bin/bash
#
# Run the whole presence chain: measure -> authenticate -> decide.
#
#   rssi-scan  |  presence-verify (root)  |  permit-bridge.sh
#   your uid      root                       your uid
#   has the radio has the key                has neither
#
# WHY THIS SCRIPT EXISTS RATHER THAN ONE PIPE
# -------------------------------------------
# The three stages cannot share a privilege level. The scanner must stay
# unprivileged: the Bluetooth TCC grant belongs to the terminal app, and the same
# binary run under root reports `STATE unauthorized` and sees nothing. The
# verifier must be root: the presence key is root-owned 0600, and it has to be,
# because anyone who can read K can mint beacons and unlock this Mac. So the
# obvious `a | b | c` cannot be written -- one shell cannot be two users.
#
# Two plain files carry the stages instead, each tailed by the next.
#
# FIFOs were the first attempt and they are a trap here. Opening one blocks until
# the other end opens, so the three stages have to be started in an order nobody
# can see from the code, and the authorization dialog sits in the middle of that
# order. Worse, `do shell script` reclaims the process group when it returns, so a
# verifier detached with nohup sometimes survived and sometimes vanished. Plain
# files have no ordering to get wrong.
#
# TWO SHAPES, BECAUSE THE TARGET DIFFERS
#
#   remote (the spike VM)  scanner | [root: verify] | bridge
#   local  (the product)   scanner | [root: verify | bridge]
#
# The only reason the bridge is kept unprivileged in the remote shape is that it
# reaches the VM over ssh with the invoking user's keys; run it as root and a
# working permit write becomes a silent authentication failure. Locally there is
# no ssh and the permit is a root-owned file on this machine, so the bridge
# belongs inside the privileged half -- which is also simpler, since it means one
# authorization prompt covers the whole chain.
#
# REPOSE_SSH being set is what selects remote. Nothing guesses.
#
# In the product the verifier becomes a launchd job holding the key and none of
# this is user-visible. This is the same shape, assembled by hand.
#
#   tools/ble-spike/mac/presence-pipeline.sh [seconds]
#
# Presence is written to the target the permit commands name -- by default the
# spike VM over ssh, so source tools/vm-spike/vm-env.sh first.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$(cd "${HERE}/../../lib" && pwd)/run-root.sh"

DURATION="${1:-0}"          # 0 = until interrupted
KEY_DIR="${REPOSE_KEY_DIR:-/var/db/repose-unlock}"
KEY_ID="${REPOSE_KEY_ID:-1}"
PERMIT_DIR="${REPOSE_PERMIT_DIR:-/var/run/repose-spike}"
STATUS_FILE_ARG="${REPOSE_STATUS_FILE:-}"
# Remote when a VM connection was handed to us; local otherwise. Explicit, so
# nobody has to infer which half the bridge is running in.
MODE="local"
[ -n "${REPOSE_SSH:-}" ] && MODE="remote"

say() { printf 'pipeline: %s\n' "$*" >&2; }

[ -e "${KEY_DIR}/presence-key.${KEY_ID}" ] \
  || { say "no presence key at ${KEY_DIR}/presence-key.${KEY_ID} -- run provision-dev-key.sh"; exit 2; }

for t in rssi-scan presence-verify; do
  src="${HERE}/${t}.swift"
  if [ ! -x "${HERE}/${t}" ] || [ "${src}" -nt "${HERE}/${t}" ]; then
    if [ "$t" = rssi-scan ]; then
      swiftc -O -o "${HERE}/${t}" "${src}" -framework CoreBluetooth || exit 2
    else
      swiftc -O -o "${HERE}/${t}" "${src}" || exit 2
    fi
    say "built ${t}"
  fi
done

WORK="${REPOSE_PIPELINE_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/repose-pipeline.XXXXXX")}"
mkdir -p "${WORK}"
RAW="${WORK}/raw.csv"
VERIFIED="${WORK}/verified.csv"
# Created by us, appended to by root. Readable so the bridge (still us) can tail
# what the verifier writes.
: > "${RAW}"; : > "${VERIFIED}"; chmod 644 "${RAW}" "${VERIFIED}"

SCAN_PID=""; ROOT_PID=""; TAIL_PID=""
cleanup() {
  for p in "${SCAN_PID}" "${ROOT_PID}" "${TAIL_PID}"; do
    [ -n "${p}" ] && kill "${p}" 2>/dev/null
  done
  # The privileged half is a child of osascript; ask root to end it.
  run_root "pkill -f 'presence-verify --key-dir' " >/dev/null 2>&1
  [ -z "${REPOSE_PIPELINE_DIR:-}" ] && rm -rf "${WORK}"
}
trap cleanup EXIT INT TERM

# 1. Scanner: unprivileged, because the Bluetooth grant belongs to the terminal
#    app and the same binary under root reports STATE unauthorized.
if [ "${DURATION}" -gt 0 ]; then
  "${HERE}/rssi-scan" --duration "${DURATION}" >> "${RAW}" 2> "${WORK}/scan.log" &
else
  "${HERE}/rssi-scan" >> "${RAW}" 2> "${WORK}/scan.log" &
fi
SCAN_PID=$!

# 2. Verifier: root, because the presence key is root-owned 0600 -- anyone who can
#    read K can mint beacons and unlock this Mac. Held in the foreground of its own
#    osascript, which is what keeps it alive.
say "starting the privileged half as root (one authorization prompt, ${MODE} target)"
if [ "${MODE}" = remote ]; then
  run_root "tail -n +1 -f '${RAW}' | '${HERE}/presence-verify' --key-dir '${KEY_DIR}' \
    >> '${VERIFIED}' 2>> '${WORK}/verify.log'" &
else
  # Local: root verifies AND decides AND writes the permit, so the permit
  # commands need no sudo and no ssh. The status file stays where the app can
  # read it, which is why it is chmod'd back afterwards -- root created it.
  run_root "REPOSE_PERMIT_ON_CMD=\"mkdir -p ${PERMIT_DIR} && chmod 755 ${PERMIT_DIR} && touch ${PERMIT_DIR}/permit\" \
    REPOSE_PERMIT_OFF_CMD=\"rm -f ${PERMIT_DIR}/permit\" \
    REPOSE_STATUS_FILE='${STATUS_FILE_ARG}' \
    sh -c \"tail -n +1 -f '${RAW}' | '${HERE}/presence-verify' --key-dir '${KEY_DIR}' 2>> '${WORK}/verify.log' | bash '${HERE}/permit-bridge.sh' 2>> '${WORK}/bridge.log'\"" &
fi
ROOT_PID=$!
sleep 3
# root may have created the status file; the app reads it unprivileged.
[ -n "${STATUS_FILE_ARG}" ] && run_root "chmod 644 '${STATUS_FILE_ARG}'" >/dev/null 2>&1

# 3. Bridge. Remote: us, so ssh uses our keys. Local: already inside the root
#    half above, so there is nothing left to start here.
say "scanner ${SCAN_PID}, logs in ${WORK}"
if [ "${MODE}" = remote ]; then
  tail -n +1 -f "${VERIFIED}" | bash "${HERE}/permit-bridge.sh"
else
  # The privileged half owns the whole chain; wait on the scanner instead.
  wait "${SCAN_PID}" 2>/dev/null
fi
