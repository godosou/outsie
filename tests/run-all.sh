#!/bin/bash
# One command to check everything the phone-unlock work depends on.
#
#   tests/run-all.sh          # fast suites, about 30 seconds
#   tests/run-all.sh --all    # adds the Swift and Android builds, a few minutes
#
# What this does NOT cover, and cannot: whether macOS loads the plugin, whether
# the phone's advertising survives Doze, or whether any of this unlocks a real
# screen. Those need the VM and the phone. A green run here means the parts we
# can check on this machine are consistent -- nothing more. The project already
# learned once what an all-green suite is worth when nothing has been proven end
# to end.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ALL=0
[ "${1:-}" = "--all" ] && ALL=1

FAILED=()
run() {
  local name="$1"; shift
  printf '\n\033[1m== %s ==\033[0m\n' "$name"
  if "$@"; then
    printf '\033[32mPASS\033[0m %s\n' "$name"
  else
    printf '\033[31mFAIL\033[0m %s\n' "$name"
    FAILED+=("$name")
  fi
}

cd "$REPO"

# The Command Line Tools ship an SDK newer than their own linker sometimes: on
# 2026-09-11 this machine had CLT executables 26.6 with MacOSX27.0.sdk selected,
# whose .tbd files declare an `arm64e.x1` architecture ld 26.6 does not know. The
# failure is a wall of linker output ending in "tapi error: malformed file",
# which reads like a broken dependency rather than a toolchain mismatch -- and it
# appears between two green runs with no source change in between.
#
# So: if the active SDK cannot link, fall back to the newest one that can, and
# say so. Picking silently would hide a real environment problem; refusing to run
# would block every Rust test on a machine whose Rust is fine.
if [ -z "${SDKROOT:-}" ] && [ -d /Library/Developer/CommandLineTools/SDKs ]; then
  active_sdk="$(xcrun --show-sdk-path 2>/dev/null)"
  if [ -n "$active_sdk" ] && grep -q 'arm64e\.x1' "$active_sdk/usr/lib/libiconv.2.tbd" 2>/dev/null; then
    for candidate in /Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
                     /Library/Developer/CommandLineTools/SDKs/MacOSX26.sdk \
                     /Library/Developer/CommandLineTools/SDKs/MacOSX15.sdk; do
      if [ -d "$candidate" ] && ! grep -q 'arm64e\.x1' "$candidate/usr/lib/libiconv.2.tbd" 2>/dev/null; then
        export SDKROOT="$candidate"
        printf 'note: the active SDK (%s) cannot be linked by the installed ld;\n' "$(basename "$active_sdk")"
        printf '      using %s instead. Update the Command Line Tools to fix properly.\n' "$(basename "$candidate")"
        break
      fi
    done
  fi
fi

run "web + lib unit tests" npm test --silent
# The health assessment lives here: which host readings mean "this Mac opens for
# anybody right now". That state is dangerous to reproduce on a real machine, so
# these are the only place it is exercised at all.
run "unlock backend health assessment" \
    env -C src-tauri cargo test --lib --quiet
run "acceptance harness self-test" tests/e2e/harness_selftest.sh
run "acceptance test passes and fails correctly" tests/e2e/fake_target_test.sh
run "authorization plugin, rule transform, install invariants" \
    make -C native/macos/minimal-auth-plugin test

# The acceptance test itself is expected to be red until a plugin is installed
# somewhere, so it is not run here. Its wiring is checked instead.
run "acceptance test wiring (dry run)" env \
    REPOSE_LEAVE_CMD='true' REPOSE_RETURN_CMD='true' \
    tests/e2e/unlock_acceptance.sh --dry-run

# Authenticated presence. The beacon verifier's arithmetic is checked against
# OpenSSL-generated vectors, and the bridge's auth gate in a sandbox. Neither
# needs a radio. What neither can check is whether the phone builds the same
# pre-image byte for byte -- only the impersonation test, with the real phone,
# does that.
run "presence beacon verifier (known-answer vectors)" \
    tools/ble-spike/mac/presence-verify --self-test
run "presence verifier behaviour" tools/ble-spike/mac/presence-verify-test.sh
run "permit bridge auth + proximity gates" tools/ble-spike/mac/permit-bridge-test.sh

# Pairing arithmetic, both languages, each against vectors OpenSSL produced --
# never against each other. Two implementations wrong the same way agree
# perfectly; a third opinion is the only thing that catches it.
# --self-test lives inside the tool that actually pairs, not beside it. A
# separate copy of the maths would be the thing under test while a different
# copy did the work -- exactly how the staleness timer passed every test and
# never ran in production.
run "pairing crypto, mac side (known-answer vectors)" \
    tools/ble-spike/mac/pair-with-phone --self-test
run "pairing crypto, phone side (same vectors, JVM)" \
    env -C tools/ble-spike/android ./gradlew --no-daemon -q testGenuineDebugUnitTest

if [ "$ALL" = "1" ]; then
  run "mac BLE scanner compiles" \
      swiftc -O tools/ble-spike/mac/rssi-scan.swift -o /tmp/rssi-scan-check \
      -framework CoreBluetooth
  run "android app builds (both flavours)" \
      env -C tools/ble-spike/android ./gradlew --no-daemon -q \
      assembleGenuineDebug assembleImposterDebug
else
  printf '\nskipped: Swift and Android builds (pass --all to include them)\n'
fi

printf '\n\033[1m== summary ==\033[0m\n'
if [ ${#FAILED[@]} -eq 0 ]; then
  echo "everything green"
  echo
  echo "Still unproven, and not provable here:"
  echo "  - does macOS load an ad-hoc signed Authorization Plugin   (needs the VM)"
  echo "  - does the phone keep advertising in Doze                 (needs the phone)"
  echo "  - has any real screen ever been unlocked by this          (needs both)"
  echo "  - has the pairing exchange ever run over the radio         (needs the phone;"
  echo "    the arithmetic agrees, the transport is not built yet)"
  echo "  - do the phone and the Mac agree on the beacon pre-image, and is an"
  echo "    unprovisioned device actually refused on the air        (needs the phone:"
  echo "      sudo -v && tests/e2e/impersonation_test.sh)"
  exit 0
fi
printf 'failed: %s\n' "${FAILED[*]}"
exit 1
