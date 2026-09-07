#!/bin/sh
set -eu

fail() {
    echo "macOS authorization artifact verification failed: $1" >&2
    exit 1
}

[ "$#" -eq 1 ] || fail "expected one artifact directory"
artifact_directory=$1
bundle="$artifact_directory/ReposeUnlock.bundle"
plist="$bundle/Contents/Info.plist"
executable="$bundle/Contents/MacOS/ReposeUnlock"
[ -d "$bundle" ] || fail "bundle is missing"
[ -f "$plist" ] || fail "Info.plist is missing"
[ -f "$executable" ] || fail "bundle executable is missing"

executable_count=$(find "$bundle/Contents/MacOS" -maxdepth 1 -type f | wc -l | tr -d ' ')
[ "$executable_count" = "1" ] || fail "bundle must contain exactly one executable"
/usr/bin/plutil -lint "$plist" >/dev/null || fail "Info.plist is invalid"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$plist")" = \
    "ReposeUnlock" ] || fail "CFBundleExecutable is invalid"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")" = \
    "ai.repose.authorization-plugin" ] || fail "CFBundleIdentifier is invalid"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundlePackageType' "$plist")" = \
    "BNDL" ] || fail "CFBundlePackageType is invalid"

/usr/bin/file "$executable" | grep -q 'Mach-O 64-bit bundle arm64' || \
    fail "executable is not an arm64 Mach-O bundle"
[ "$(/usr/bin/lipo -archs "$executable")" = "arm64" ] || \
    fail "bundle must be arm64-only"
minimum_os=$(/usr/bin/otool -l "$executable" | awk '
    $1 == "cmd" && $2 == "LC_BUILD_VERSION" { build = 1; next }
    build && $1 == "minos" { print $2; exit }
')
[ "$minimum_os" = "14.0" ] || fail "minimum macOS version must be 14.0"

/usr/bin/codesign --verify --strict --verbose=2 "$bundle" || \
    fail "code signature is invalid"
signature=$(/usr/bin/codesign -dvv "$bundle" 2>&1)
printf '%s\n' "$signature" | grep -q '^Signature=adhoc$' || \
    fail "bundle is not ad-hoc signed"
entitlements=$(/usr/bin/codesign -d --entitlements - "$executable" 2>&1 || true)
if printf '%s\n' "$entitlements" | grep -q '<plist'; then
    fail "bundle must not carry entitlements"
fi

exports=$(/usr/bin/nm -gjU "$executable")
[ "$exports" = "_AuthorizationPluginCreate" ] || \
    fail "AuthorizationPluginCreate must be the only exported symbol"
if /usr/bin/strings "$executable" | grep -E \
    'REPOSE_UNLOCK_TESTING|repose_(plugin|ipc)_test|/tmp/repose|AllowAll|ALLOW_ALL' \
    >/dev/null; then
    fail "bundle contains a test or allow-all seam"
fi
/usr/bin/strings "$executable" | \
    grep -q '^/var/run/ai.repose.unlockd/consume.sock$' || \
    fail "bundle does not contain the fixed production socket path"

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_directory/.." && pwd)
plugin_directory="$repository_root/native/macos/authorization-plugin"
make -C "$plugin_directory" build/bundle_smoke >/dev/null
"$plugin_directory/build/bundle_smoke" "$executable" >/dev/null

echo "macOS authorization artifacts: ok"
