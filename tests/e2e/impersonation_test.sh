#!/bin/bash
# Can the Mac tell our phone from a device that is only pretending to be it?
#
# This is the test E13 was written against. Presence used to be decided by a
# service UUID and a fixed string, both published in this repository, so the
# cheapest attack was not writing an app -- it was installing a second copy of
# ours under a different package id. That is the `imposter` build flavour, and
# until the presence beacon existed this test necessarily failed.
#
# IT TAKES BOTH LEGS TO MEAN ANYTHING
# -----------------------------------
# "The imposter was rejected" is trivially satisfiable by rejecting everything --
# an unplugged antenna passes that. So the same run also requires the genuine
# phone to be accepted, with the same binaries, the same key, and the same
# seconds-apart conditions. The claim under test is discrimination, not refusal,
# and only the two legs together state it.
#
# Needs: the phone on adb, Bluetooth on, the Mac's Bluetooth permission granted
# to this terminal, and a provisioned key (tools/ble-spike/provision-dev-key.sh).
# Touches no Mac system state beyond reading the key file, and installs nothing
# on the Mac. It does install and then uninstall the imposter on the phone.
#
#   tests/e2e/impersonation_test.sh [seconds-per-leg]     (default 20)

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${HERE}/../.." && pwd)"
ANDROID="${REPO}/tools/ble-spike/android"
MAC="${REPO}/tools/ble-spike/mac"
DURATION="${1:-20}"
KEY_ID="${REPOSE_KEY_ID:-1}"
KEY_FILE="${REPOSE_KEY_DIR:-/var/db/repose-unlock}/presence-key.${KEY_ID}"

GENUINE="ai.repose.blespike"
IMPOSTER="ai.repose.imposter"
ACTIVITY="ai.repose.blespike.MainActivity"

PASS=0
FAIL=0
ok() { printf '  ok   %s\n' "$1"; PASS=$((PASS + 1)); }
no() { printf '  FAIL %s -- %s\n' "$1" "${2:-}"; FAIL=$((FAIL + 1)); }
note() { printf '  ..   %s\n' "$1"; }
void() { printf '\n  VOID: %s\n' "$1" >&2; exit 2; }

SERIAL="${REPOSE_PHONE:-$(adb devices 2>/dev/null | awk 'NR>1 && $2=="device" && $1 !~ /emulator/ {print $1; exit}')}"
[ -n "$SERIAL" ] || void "no phone on adb; connect it or set REPOSE_PHONE"

adbs() { adb -s "$SERIAL" "$@"; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/imp.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "impersonation test"
echo "  phone   ${SERIAL}"
echo "  key     ${KEY_FILE}"
echo "  scan    ${DURATION}s per leg"
echo

# --- preflight: everything that would make a result meaningless ---------------

sudo -n true 2>/dev/null || {
  echo "  The verifier reads a root-only key file, so this test needs sudo."
  echo "  Run 'sudo -v' first, then rerun."
  void "no sudo credential cached"
}
sudo test -f "$KEY_FILE" || void "no presence key at ${KEY_FILE}; run tools/ble-spike/provision-dev-key.sh"

for target in rssi-scan presence-verify; do
  src="${MAC}/${target}.swift"
  if [ ! -x "${MAC}/${target}" ] || [ "$src" -nt "${MAC}/${target}" ]; then
    # Not an array: /bin/bash here is 3.2, where expanding an empty one under
    # `set -u` is an error rather than an empty list.
    if [ "$target" = rssi-scan ]; then
      swiftc -O -o "${MAC}/${target}" "$src" -framework CoreBluetooth \
        || void "could not build ${target}"
    else
      swiftc -O -o "${MAC}/${target}" "$src" || void "could not build ${target}"
    fi
    note "built ${target}"
  fi
done

# If the verifier's own arithmetic is wrong, both legs fail and the run would
# read as a spectacular success.
"${MAC}/presence-verify" --self-test > "${WORK}/selftest.log" 2>&1 \
  || { cat "${WORK}/selftest.log"; void "the verifier's known-answer self-test failed"; }
note "verifier self-test passed"

IMP_APK="${ANDROID}/app/build/outputs/apk/imposter/debug/app-imposter-debug.apk"
[ -f "$IMP_APK" ] || void "build it first: (cd ${ANDROID} && ./gradlew assembleImposterDebug)"

# --- one leg: run only the named package and see what the Mac makes of it -----

# $1 = package, $2 = label. Leaves the annotated CSV in ${WORK}/$2.csv
run_leg() {
  local pkg="$1" label="$2"
  adbs shell am force-stop "$GENUINE" >/dev/null 2>&1
  adbs shell am force-stop "$IMPOSTER" >/dev/null 2>&1
  sleep 2
  adbs shell am start -n "${pkg}/${ACTIVITY}" --ez autostart true >/dev/null 2>&1
  sleep 4

  "${MAC}/rssi-scan" --duration "$DURATION" 2> "${WORK}/${label}.scan.log" \
    | sudo "${MAC}/presence-verify" --key-dir "$(dirname "$KEY_FILE")" \
        > "${WORK}/${label}.csv" 2> "${WORK}/${label}.verify.log"
}

count() { grep -c ",$1\$" "${WORK}/${2}.csv" 2>/dev/null; true; }
rows()  { [ -f "${WORK}/${1}.csv" ] && wc -l < "${WORK}/${1}.csv" | tr -d ' '; }

# --- leg 1: the real phone must be accepted ----------------------------------

echo "leg 1: the genuine app"
run_leg "$GENUINE" genuine
G_ROWS="$(rows genuine)"; G_VALID="$(count VALID genuine)"; G_INVALID="$(count INVALID genuine)"
G_NOKEY="$(count NOKEY genuine)"
note "rows=${G_ROWS} VALID=${G_VALID} INVALID=${G_INVALID} NOKEY=${G_NOKEY}"

if [ "${G_ROWS:-0}" -eq 0 ]; then
  void "the genuine phone never got on the air (0 advertisements seen). Open the app,
        allow Bluetooth, and confirm the beacon is running."
fi
if [ "${G_NOKEY}" -gt 0 ]; then
  sed -n '1,3p' "${WORK}/genuine.verify.log" >&2
  void "the Mac holds no usable key for the keyId the phone is broadcasting"
fi
[ "${G_VALID}" -gt 0 ] \
  && ok "the paired phone is accepted (${G_VALID} verified beacons)" \
  || no "the paired phone is accepted" \
        "0 of ${G_ROWS} beacons verified -- phone and Mac hold different keys, or their
         clocks differ by more than a window"

# --- leg 2: a device we never provisioned must not be -------------------------

echo
echo "leg 2: the imposter"
adbs install -r -t "$IMP_APK" >/dev/null 2>&1 || void "could not install the imposter"
note "imposter installed as ${IMPOSTER}"

# Runtime permissions cannot be granted over adb on this OEM build. Without them
# the imposter never advertises, which is indistinguishable from an imposter that
# was correctly refused -- so this refuses to report anything at all.
PERMS_OK=1
for p in BLUETOOTH_ADVERTISE BLUETOOTH_CONNECT; do
  adbs shell dumpsys package "$IMPOSTER" 2>/dev/null \
    | grep -q "android.permission.${p}: granted=true" || PERMS_OK=0
done
if [ "$PERMS_OK" != "1" ]; then
  echo
  echo "  The imposter has no Bluetooth permission yet. Open 'BLE Imposter' on the"
  echo "  phone once and allow the prompts, then rerun."
  void "an imposter that never advertised proves nothing"
fi

run_leg "$IMPOSTER" imposter
I_ROWS="$(rows imposter)"; I_VALID="$(count VALID imposter)"; I_INVALID="$(count INVALID imposter)"
note "rows=${I_ROWS} VALID=${I_VALID} INVALID=${I_INVALID}"

if [ "${I_ROWS:-0}" -eq 0 ]; then
  void "the imposter never got on the air (0 advertisements). Zero samples is not a
        pass: it means the attack was never attempted."
fi
[ "${I_VALID}" -eq 0 ] \
  && ok "an unprovisioned device is refused (${I_INVALID} beacons, none verified)" \
  || no "an unprovisioned device is refused" \
        "${I_VALID} of ${I_ROWS} imposter beacons verified -- presence still has no identity"

# --- the claim, stated once ---------------------------------------------------

echo
if [ "${G_VALID}" -gt 0 ] && [ "${I_VALID}" -eq 0 ]; then
  ok "the Mac can tell the two apart"
else
  no "the Mac can tell the two apart" \
     "genuine VALID=${G_VALID} (want >0), imposter VALID=${I_VALID} (want 0)"
fi

adbs shell am force-stop "$IMPOSTER" >/dev/null 2>&1
adbs uninstall "$IMPOSTER" >/dev/null 2>&1 && note "imposter uninstalled"
adbs shell am start -n "${GENUINE}/${ACTIVITY}" --ez autostart true >/dev/null 2>&1

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
