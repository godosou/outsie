#!/bin/bash
#
# Pair this Mac with the phone, using repose-pair-v2.
#
#   tools/ble-spike/pair.sh
#
# On the phone: 密钥状态 → 开始配对. Then run this. Both screens show six
# digits; they must match. Confirm on both and the key is written here.
#
# WHY THE KEY TRAVELS THROUGH A PIPE AND A FILE, NOT ONE PROGRAM
#
# pair-with-phone talks to a stranger over a radio. Writing the key needs root.
# Putting both in one process means the thing handling untrusted input runs
# privileged, which is a worse shape than handing 64 hex characters between two
# programs. So the pairing tool prints the key and nothing else on stdout, and
# this script -- which touches no radio -- is what asks for root.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "${HERE}/../lib/run-root.sh"

KEY_ID="${REPOSE_KEY_ID:-1}"
KEY_DIR="${REPOSE_KEY_DIR:-/var/db/repose-unlock}"
KEY_FILE="${KEY_DIR}/presence-key.${KEY_ID}"
TOOL="${HERE}/mac/pair-with-phone"

[ -x "${TOOL}" ] || swiftc -O -o "${TOOL}" "${TOOL}.swift" -framework CoreBluetooth || {
    echo "pair: could not build pair-with-phone" >&2; exit 2; }

TMP="$(mktemp -t repose-pair)"
# 0600 before anything is written: the key is in the clear here for the moment
# between pairing and installing it, and that moment should not be readable by
# anything else on the machine.
chmod 600 "${TMP}"
trap 'rm -f "${TMP}"' EXIT

"${TOOL}" > "${TMP}"
rc=$?
[ "${rc}" = 0 ] || exit "${rc}"

K="$(tr -d '[:space:]' < "${TMP}")"
[ "${#K}" = 64 ] || { echo "pair: expected 64 hex characters, got ${#K}" >&2; exit 2; }

fingerprint() {
  { printf 'repose-presence-v1 fingerprint'; printf '%s' "$1" | xxd -r -p; } \
    | shasum -a 256 | cut -c1-8 | tr 'a-f' 'A-F'
}
FP="$(fingerprint "${K}")"

echo "  写入这台 Mac（需要一次管理员授权）…" >&2
run_root "mkdir -p '${KEY_DIR}' && chown root:wheel '${KEY_DIR}' && chmod 755 '${KEY_DIR}' \
  && install -m 600 -o root -g wheel /dev/null '${KEY_FILE}' \
  && printf '%s\n' '${K}' > '${KEY_FILE}' && chmod 600 '${KEY_FILE}'" \
  || { echo "pair: could not write ${KEY_FILE}" >&2; exit 2; }

echo "" >&2
echo "  完成。密钥指纹 ${FP}" >&2
echo "  手机上的「密钥状态」应当显示同一串。不一致就说明写错了机器。" >&2
