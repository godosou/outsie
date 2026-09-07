#!/bin/sh
set -eu
LC_ALL=C
export LC_ALL

usage() {
    echo "usage: $0 --unsigned --output target/<directory>" >&2
    exit 2
}

mode=
output=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --unsigned)
            [ -z "$mode" ] || usage
            mode=adhoc-development
            shift
            ;;
        --output)
            [ "$#" -ge 2 ] || usage
            [ -z "$output" ] || usage
            output=$2
            shift 2
            ;;
        *) usage ;;
    esac
done

[ "$mode" = "adhoc-development" ] || usage
[ -n "$output" ] || usage
[ "$(id -u)" -ne 0 ] || {
    echo "refusing to build a plan-only package as root" >&2
    exit 1
}
case "$output" in
    target/*) ;;
    *) usage ;;
esac
case "/$output/" in
    *"/../"*|*"/./"*) usage ;;
esac

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_directory/.." && pwd)
output_path="$repository_root/$output"
[ ! -e "$output_path" ] || {
    echo "refusing to replace existing package output: $output_path" >&2
    exit 1
}

output_parent=$(dirname -- "$output_path")
mkdir -p -- "$output_parent"
staging=$(mktemp -d "$output_parent/.repose-package.XXXXXX")
cleanup() {
    if [ -n "${staging:-}" ] && [ -d "$staging" ]; then
        rm -rf -- "$staging"
    fi
}
trap cleanup EXIT HUP INT TERM

"$script_directory/build-macos-auth-prototype.sh" \
    --configuration release --arch arm64 --sign ad-hoc
cargo build --offline --manifest-path "$repository_root/src-tauri/Cargo.toml" \
    --release -p repose-unlock-service
health=$(env -i PATH=/usr/bin:/bin \
    "$repository_root/src-tauri/target/release/repose-unlock-service" \
    --health-check-deny-only 2>/dev/null) || {
    echo "locally built service health command failed" >&2
    exit 1
}
[ "$health" = "repose-unlock-service: deny-only-v1" ] || {
    echo "locally built service health response is invalid" >&2
    exit 1
}

mkdir -p -- "$staging/bin" "$staging/launchd"
/usr/bin/ditto --noextattr --noqtn \
    "$repository_root/target/macos-auth/release/ReposeUnlock.bundle" \
    "$staging/ReposeUnlock.bundle"
cp -- "$repository_root/src-tauri/target/release/repose-unlock-service" \
    "$staging/bin/ai.repose.unlockd"
cp -- "$repository_root/native/macos/launchd/ai.repose.unlockd.plist" \
    "$staging/launchd/ai.repose.unlockd.plist"

/usr/bin/codesign --force --sign - --timestamp=none \
    "$staging/bin/ai.repose.unlockd"
/usr/bin/codesign --verify --strict --verbose=2 \
    "$staging/bin/ai.repose.unlockd"

find "$staging" -type d -exec chmod 0755 {} \;
find "$staging" -type f -exec chmod 0644 {} \;
chmod 0755 \
    "$staging/ReposeUnlock.bundle/Contents/MacOS/ReposeUnlock" \
    "$staging/bin/ai.repose.unlockd"

manifest="$staging/SHA256SUMS"
{
    echo "repose-package-v1"
    echo "mode=adhoc-development"
    for relative in \
        ReposeUnlock.bundle/Contents/Info.plist \
        ReposeUnlock.bundle/Contents/MacOS/ReposeUnlock \
        ReposeUnlock.bundle/Contents/_CodeSignature/CodeResources \
        bin/ai.repose.unlockd \
        launchd/ai.repose.unlockd.plist
    do
        digest=$(/usr/bin/shasum -a 256 "$staging/$relative" | awk '{print $1}')
        echo "$digest  $relative"
    done
} >"$manifest"
chmod 0644 "$manifest"

"$script_directory/verify-macos-auth-artifacts.sh" "$staging"
mv -- "$staging" "$output_path"
staging=
trap - EXIT HUP INT TERM
echo "packaged plan-only ad-hoc components at $output_path"
