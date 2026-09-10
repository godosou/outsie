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
# Needs adb (to reach the phone) and one administrator authorization (to write the
# Mac's root-only key file). macOS asks for that with its own dialog; no terminal
# and no cached sudo credential are required.

set -uo pipefail

KEY_ID="${REPOSE_KEY_ID:-1}"
KEY_DIR="${REPOSE_KEY_DIR:-/var/db/repose-unlock}"
KEY_FILE="${KEY_DIR}/presence-key.${KEY_ID}"
PKG="${REPOSE_PKG:-ai.repose.blespike}"
PHONE_DROP="/sdcard/Android/data/${PKG}/files/presence-key.hex"

say() { printf '%s\n' "$*"; }
die() { printf 'provision: %s\n' "$*" >&2; exit 1; }

# Root without a terminal: see the file for why.
. "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/run-root.sh"

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
  # autostart, or the app comes up showing a screen and never begins advertising --
  # which looks exactly like a key that failed to import.
  adb -s "${SERIAL}" shell am start -n "${PKG}/ai.repose.blespike.MainActivity" \
    --ez autostart true >/dev/null 2>&1
  say "  restarted the app so the service picks the key up"
}

case "${1:-}" in
  --show)
    if [ -e "${KEY_FILE}" ]; then
      # Ownership and mode are readable without root; the key itself is not, and
      # this tool has no reason to read it.
      say "mac:   $(stat -f '%N (%Su:%Sg %Lp)' "${KEY_FILE}")"
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
    run_root "rm -f '${KEY_FILE}'" && say "removed ${KEY_FILE}"
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
# The file is created 0600 root:wheel BEFORE the bytes go in, not after. A key
# that is briefly world-readable is a key that was briefly readable, and the
# verifier's ownership check cannot catch that -- by the time anything reads the
# file the window has closed.
#
# One privileged command, not five: each run_root may cost an authorization
# dialog, and a script that asks five times trains people to click through.
say "  (macOS will ask for your password once, to write a root-only key file)"
run_root "mkdir -p '${KEY_DIR}' \
  && chown root:wheel '${KEY_DIR}' && chmod 755 '${KEY_DIR}' \
  && install -m 600 -o root -g wheel /dev/null '${KEY_FILE}' \
  && printf '%s\\n' '${K_HEX}' > '${KEY_FILE}' \
  && chmod 600 '${KEY_FILE}'" || die "could not write ${KEY_FILE} (was the prompt cancelled?)"
stat -f '  mac:   %N (%Su:%Sg %Lp)' "${KEY_FILE}" 2>/dev/null \
  || die "${KEY_FILE} was not created"

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
