#!/bin/bash
# Can a device we never paired with pass itself off as the phone?
#
# Presence is currently decided by a service UUID and a fixed payload, both of
# which are published in this repository. So the cheapest attack is not writing
# an app -- it is installing a second copy of ours under a different package id.
# That is exactly what the "imposter" build flavour is.
#
# THIS TEST IS EXPECTED TO FAIL TODAY. Its failure is the evidence for E13. It
# flips to passing on the day the Mac can tell the two apart, and that flip is
# the only proof a device-identity fix actually works. A fix landing without
# this test turning green has not been demonstrated, only asserted.
#
# Needs: the phone on adb, Bluetooth on, and the Mac's Bluetooth permission
# already granted to this terminal. Touches no Mac system state and installs
# nothing on the Mac.
#
#   tests/e2e/impersonation_test.sh [seconds]     (default 25)

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${HERE}/../.." && pwd)"
ANDROID="${REPO}/tools/ble-spike/android"
MAC="${REPO}/tools/ble-spike/mac"
DURATION="${1:-25}"

GENUINE="ai.repose.blespike"
IMPOSTER="ai.repose.imposter"

PASS=0
FAIL=0
ok() { printf '  ok   %s\n' "$1"; PASS=$((PASS + 1)); }
no() { printf '  FAIL %s -- %s\n' "$1" "${2:-}"; FAIL=$((FAIL + 1)); }
note() { printf '  ..   %s\n' "$1"; }

SERIAL="${REPOSE_PHONE:-$(adb devices 2>/dev/null | awk 'NR>1 && $2=="device" && $1 !~ /emulator/ {print $1; exit}')}"
[ -n "$SERIAL" ] || { echo "no phone on adb; connect it or set REPOSE_PHONE" >&2; exit 2; }

adbs() { adb -s "$SERIAL" "$@"; }

echo "impersonation test"
echo "  phone   ${SERIAL}"
echo "  scan    ${DURATION}s"
echo

# --- make sure only the imposter is on the air ------------------------------

adbs shell am force-stop "$GENUINE" >/dev/null 2>&1
adbs shell am force-stop "$IMPOSTER" >/dev/null 2>&1
sleep 2

IMP_APK="${ANDROID}/app/build/outputs/apk/imposter/debug/app-imposter-debug.apk"
[ -f "$IMP_APK" ] || { echo "build it first: (cd ${ANDROID} && ./gradlew assembleImposterDebug)" >&2; exit 2; }
adbs install -r -t "$IMP_APK" >/dev/null 2>&1 || { echo "could not install the imposter" >&2; exit 2; }
note "imposter installed as ${IMPOSTER}"

# Runtime permissions cannot be granted over adb on this OEM build, so the
# imposter needs them tapped through once. If they are missing, say so rather
# than reporting a pass that only means the imposter never got on the air.
PERMS_OK=1
for p in BLUETOOTH_ADVERTISE BLUETOOTH_CONNECT; do
  adbs shell dumpsys package "$IMPOSTER" 2>/dev/null \
    | grep -q "android.permission.${p}: granted=true" || PERMS_OK=0
done
if [ "$PERMS_OK" != "1" ]; then
  echo
  echo "  The imposter does not yet have Bluetooth permission. Open 'BLE Imposter'"
  echo "  on the phone once, allow the prompts, tap START ADVERTISING, then rerun."
  echo "  Refusing to report a result: an imposter that never advertised would"
  echo "  look identical to an imposter that was correctly rejected."
  exit 2
fi

adbs shell am start -n "${IMPOSTER}/ai.repose.blespike.MainActivity" >/dev/null 2>&1
sleep 2
# Tap START ADVERTISING by its label, so a layout change does not silently turn
# this into a test of an app that is not advertising.
adbs shell uiautomator dump /sdcard/imp.xml >/dev/null 2>&1
BOUNDS="$(adbs shell cat /sdcard/imp.xml 2>/dev/null \
  | tr '<' '\n' | grep -i 'START ADVERTISING' | grep -o 'bounds="[^"]*"' | head -1 \
  | sed 's/bounds="//; s/"//')"
if [ -n "$BOUNDS" ]; then
  X=$(echo "$BOUNDS" | sed 's/\[\([0-9]*\),\([0-9]*\)\]\[\([0-9]*\),\([0-9]*\)\]/\1 \3/' | awk '{print int(($1+$2)/2)}')
  Y=$(echo "$BOUNDS" | sed 's/\[\([0-9]*\),\([0-9]*\)\]\[\([0-9]*\),\([0-9]*\)\]/\2 \4/' | awk '{print int(($1+$2)/2)}')
  adbs shell input tap "$X" "$Y" >/dev/null 2>&1
  note "tapped START ADVERTISING at ${X},${Y}"
else
  note "could not locate the button; assuming it is already advertising"
fi
sleep 3

# --- what does the Mac make of it? ------------------------------------------

[ -x "${MAC}/rssi-scan" ] || swiftc -O -o "${MAC}/rssi-scan" "${MAC}/rssi-scan.swift" \
  -framework CoreBluetooth >/dev/null 2>&1
OUT="$(mktemp -t imp-scan)"; ERR="$(mktemp -t imp-scan-log)"
trap 'rm -f "$OUT" "$ERR"' EXIT

"${MAC}/rssi-scan" --duration "$DURATION" > "$OUT" 2> "$ERR"

SAMPLES="$(wc -l < "$OUT" | tr -d ' ')"
ACCEPTED=0
grep -q "READ MATCH" "$ERR" && ACCEPTED=1

echo
note "samples from the imposter: ${SAMPLES}"
if [ "$ACCEPTED" = "1" ]; then
  note "scanner verdict: accepted -- $(grep -m1 'READ MATCH' "$ERR" | sed 's/.*READ/READ/')"
else
  note "scanner verdict: not accepted"
fi

# --- the assertion ----------------------------------------------------------

if [ "$SAMPLES" -eq 0 ]; then
  no "the imposter got on the air at all" \
     "zero samples: the test proved nothing, it did not pass"
elif [ "$ACCEPTED" = "1" ]; then
  no "an unpaired device is rejected" \
     "the scanner accepted a device it has never paired with; presence has no identity (E13)"
else
  ok "an unpaired device is rejected"
fi

# Leave the phone as we found it.
adbs shell am force-stop "$IMPOSTER" >/dev/null 2>&1
adbs uninstall "$IMPOSTER" >/dev/null 2>&1 && note "imposter uninstalled"

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
