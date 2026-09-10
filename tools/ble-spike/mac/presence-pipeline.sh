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
# The bridge deliberately stays as the invoking user. It reaches the target over
# ssh, and root has different ssh keys -- running it privileged turns a working
# permit write into a silent authentication failure.
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
say "starting the verifier as root (one authorization prompt)"
run_root "tail -n +1 -f '${RAW}' | '${HERE}/presence-verify' --key-dir '${KEY_DIR}' \
  >> '${VERIFIED}' 2>> '${WORK}/verify.log'" &
ROOT_PID=$!
sleep 3

# 3. Bridge: us again. Holds no key and no radio; only decides near/far. It is the
#    foreground process, so the run ends when it does.
say "scanner ${SCAN_PID}, logs in ${WORK}"
tail -n +1 -f "${VERIFIED}" | bash "${HERE}/permit-bridge.sh"
