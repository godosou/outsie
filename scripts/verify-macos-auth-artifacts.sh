#!/bin/sh
set -eu
LC_ALL=C
export LC_ALL

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

service="$artifact_directory/bin/ai.repose.unlockd"
launchd_plist="$artifact_directory/launchd/ai.repose.unlockd.plist"
manifest="$artifact_directory/SHA256SUMS"
if [ -e "$service" ] || [ -e "$launchd_plist" ] || [ -e "$manifest" ]; then
    [ -f "$service" ] && [ ! -L "$service" ] || fail "service is missing or a symlink"
    [ -f "$launchd_plist" ] && [ ! -L "$launchd_plist" ] || \
        fail "launchd plist is missing or a symlink"
    [ -f "$manifest" ] && [ ! -L "$manifest" ] || \
        fail "package manifest is missing or a symlink"
    cmp -s "$launchd_plist" \
        "$repository_root/native/macos/launchd/ai.repose.unlockd.plist" || \
        fail "launchd plist differs from the fixed installer template"
    /usr/bin/file "$service" | grep -q 'Mach-O 64-bit executable arm64' || \
        fail "service is not an arm64 Mach-O executable"
    [ "$(/usr/bin/lipo -archs "$service")" = "arm64" ] || \
        fail "service must be arm64-only"
    /usr/bin/codesign --verify --strict --verbose=2 "$service" || \
        fail "service code signature is invalid"
    service_signature=$(/usr/bin/codesign -dvv "$service" 2>&1)
    printf '%s\n' "$service_signature" | grep -q '^Signature=adhoc$' || \
        fail "service is not ad-hoc signed"
    [ "$(sed -n '1p' "$manifest")" = "repose-package-v1" ] || \
        fail "package manifest version is invalid"
    [ "$(sed -n '2p' "$manifest")" = "mode=adhoc-development" ] || \
        fail "package must be marked plan-only ad-hoc"
    [ "$(wc -l <"$manifest" | tr -d ' ')" = "7" ] || \
        fail "package manifest entry count is invalid"
    for relative in \
        ReposeUnlock.bundle/Contents/Info.plist \
        ReposeUnlock.bundle/Contents/MacOS/ReposeUnlock \
        ReposeUnlock.bundle/Contents/_CodeSignature/CodeResources \
        bin/ai.repose.unlockd \
        launchd/ai.repose.unlockd.plist
    do
        expected=$(awk -v path="$relative" '$2 == path { print $1 }' "$manifest")
        [ "${#expected}" = "64" ] || fail "missing package digest for $relative"
        actual=$(/usr/bin/shasum -a 256 "$artifact_directory/$relative" | awk '{print $1}')
        [ "$actual" = "$expected" ] || fail "package digest mismatch for $relative"
    done
    package_file_count=$(find "$artifact_directory" -type f | wc -l | tr -d ' ')
    [ "$package_file_count" = "6" ] || fail "package contains unexpected files"
    if find "$artifact_directory" -type l | grep -q .; then
        fail "package contains a symlink"
    fi
fi

echo "macOS authorization artifacts: ok"
