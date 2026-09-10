#!/bin/bash
#
# Put one presence key on this Mac and on the phone plugged into it.
#
# WHAT THIS IS NOT
# ----------------
# This is not pairing. Pairing -- the SAS-guarded exchange in
# docs/plans/2026-09-10-authenticated-presence-design.md §1, where both screens show
# six digits and a human compares them -- defends against an attacker sitting in the
# middle of the exchange. This script has no such defence and does not need one: it
# assumes whoever is holding the USB cable owns both devices.
#
# So this closes exactly one hole -- a stranger's device can no longer pass itself off
# as your phone -- and leaves the bootstrap for later. Saying that plainly matters
# here: three separate artifacts on this project have described a safeguard the code
# did not have, and a script called "pair" that did this would be the fourth.
#
# The key does go where a real key goes: root-only 0600 on the Mac, non-exportable
# inside AndroidKeyStore on the phone. Only the way it gets there is a shortcut.
#
#   tools/ble-spike/provision-dev-key.sh            # provision both ends
#   tools/ble-spike/provision-dev-key.sh --show     # print what is provisioned
#   tools/ble-spike/provision-dev-key.sh --revoke   # remove both ends
#
# Needs sudo (to write the Mac's key file) and adb (to reach the phone).

set -uo pipefail

KEY_ID="${REPOSE_KEY_ID:-1}"
KEY_DIR="${REPOSE_KEY_DIR:-/var/db/repose-unlock}"
KEY_FILE="${KEY_DIR}/presence-key.${KEY_ID}"
PKG="${REPOSE_PKG:-ai.repose.blespike}"
PHONE_DROP="/sdcard/Android/data/${PKG}/files/presence-key.hex"

say() { printf '%s\n' "$*"; }
die() { printf 'provision: %s\n' "$*" >&2; exit 1; }

SERIAL="${REPOSE_PHONE:-$(adb devices 2>/dev/null | awk 'NR>1 && $2=="device" && $1 !~ /emulator/ {print $1; exit}')}"

# The fingerprint the phone will show. Same domain label as PresenceKey.fingerprintOf,
# so the two strings are comparable; a mismatch means the two ends hold different keys.
fingerprint() {
  local k_hex="$1"
  { printf 'repose-presence-v1 fingerprint'; printf '%s' "${k_hex}" | xxd -r -p; } \
    | shasum -a 256 | cut -c1-8 | tr 'a-f' 'A-F'
}

restart_beacon() {
  [ -n "${SERIAL}" ] || return 0
  adb -s "${SERIAL}" shell am force-stop "${PKG}" >/dev/null 2>&1
  adb -s "${SERIAL}" shell am start -n "${PKG}/ai.repose.blespike.MainActivity" >/dev/null 2>&1
  say "  restarted the app so the service picks the key up"
}

case "${1:-}" in
  --show)
    if sudo test -f "${KEY_FILE}"; then
      k="$(sudo cat "${KEY_FILE}" | tr -d '[:space:]')"
      say "mac:   ${KEY_FILE}  fingerprint $(fingerprint "${k}")"
      say "       $(sudo stat -f 'owner=%Su group=%Sg mode=%Lp' "${KEY_FILE}")"
    else
      say "mac:   no key at ${KEY_FILE}"
    fi
    if [ -n "${SERIAL}" ]; then
      say "phone: ${SERIAL} — open the app's 密钥状态 screen to read its fingerprint"
    else
      say "phone: none on adb"
    fi
    exit 0
    ;;
  --revoke)
    sudo rm -f "${KEY_FILE}" && say "removed ${KEY_FILE}"
    if [ -n "${SERIAL}" ]; then
      # Clearing app data is the only way to reach a Keystore entry from here. It
      # takes the fingerprint record with it, which is what we want: a phone that
      # kept showing a fingerprint for a key the Mac has dropped would be lying.
      adb -s "${SERIAL}" shell pm clear "${PKG}" >/dev/null 2>&1 \
        && say "cleared ${PKG} on ${SERIAL} (Keystore entry gone)"
    fi
    say "the phone will now broadcast an invalid tag, and the Mac will refuse it."
    exit 0
    ;;
  "") ;;
  *) die "unknown option: $1" ;;
esac

[ -n "${SERIAL}" ] || die "no phone on adb; connect it or set REPOSE_PHONE"

K_HEX="$(openssl rand -hex 32)"
[ "${#K_HEX}" = 64 ] || die "could not generate 32 random bytes"
FP="$(fingerprint "${K_HEX}")"

say "provisioning presence key ${KEY_ID}, fingerprint ${FP}"

# --- Mac end ----------------------------------------------------------------
# Written 0600 before the bytes go in, not after: a key that is briefly world-readable
# is a key that was briefly readable, and the verifier's ownership check would not
# catch that because it is over by the time anything reads it.
sudo mkdir -p "${KEY_DIR}" || die "could not create ${KEY_DIR}"
sudo chown root:wheel "${KEY_DIR}" && sudo chmod 755 "${KEY_DIR}"
sudo install -m 600 -o root -g wheel /dev/null "${KEY_FILE}" || die "could not create ${KEY_FILE}"
printf '%s\n' "${K_HEX}" | sudo tee "${KEY_FILE}" >/dev/null || die "could not write ${KEY_FILE}"
sudo chmod 600 "${KEY_FILE}"
say "  mac:   ${KEY_FILE} ($(sudo stat -f '%Su:%Sg %Lp' "${KEY_FILE}"))"

# --- phone end --------------------------------------------------------------
TMP="$(mktemp -t presence-key)"
trap 'rm -f "${TMP}"' EXIT
printf '%s\n' "${K_HEX}" > "${TMP}"
adb -s "${SERIAL}" shell "mkdir -p /sdcard/Android/data/${PKG}/files" >/dev/null 2>&1
adb -s "${SERIAL}" push "${TMP}" "${PHONE_DROP}" >/dev/null 2>&1 \
  || die "could not push the key to ${SERIAL} (is the app installed?)"
say "  phone: pushed to ${SERIAL}; the app imports it into Keystore and deletes the file"

restart_beacon

say ""
say "check both ends show ${FP}:"
say "  mac    tools/ble-spike/provision-dev-key.sh --show"
say "  phone  open the app, 密钥状态 screen"
