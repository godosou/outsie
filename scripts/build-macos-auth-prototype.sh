#!/bin/sh
set -eu

usage() {
    echo "usage: $0 --configuration <debug|release> --arch arm64 --sign ad-hoc" >&2
    exit 2
}

configuration=
architecture=
signing=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --configuration)
            [ "$#" -ge 2 ] || usage
            configuration=$2
            shift 2
            ;;
        --arch)
            [ "$#" -ge 2 ] || usage
            architecture=$2
            shift 2
            ;;
        --sign)
            [ "$#" -ge 2 ] || usage
            signing=$2
            shift 2
            ;;
        *)
            usage
            ;;
    esac
done

case "$configuration" in
    debug|release) ;;
    *) usage ;;
esac
[ "$architecture" = "arm64" ] || usage
[ "$signing" = "ad-hoc" ] || usage

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_directory/.." && pwd)
plugin_directory="$repository_root/native/macos/authorization-plugin"
output_directory="$repository_root/target/macos-auth/$configuration"
bundle="$output_directory/ReposeUnlock.bundle"

if [ -e "$bundle" ]; then
    rm -rf -- "$bundle"
fi
if [ -e "$output_directory/ReposeUnlock.bundle.dSYM" ]; then
    rm -rf -- "$output_directory/ReposeUnlock.bundle.dSYM"
fi
make -C "$plugin_directory" bundle \
    CONFIGURATION="$configuration" \
    ARCH="$architecture" \
    BUNDLE_OUTPUT_DIR="$output_directory"
/usr/bin/codesign --force --sign - --timestamp=none "$bundle"
/usr/bin/codesign --verify --strict --verbose=2 "$bundle"
make -C "$plugin_directory" build/bundle_smoke
"$plugin_directory/build/bundle_smoke" \
    "$bundle/Contents/MacOS/ReposeUnlock"

echo "built $bundle"
