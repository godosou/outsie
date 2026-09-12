#!/bin/sh
# Prints every "is a meeting going on" signal every 3 seconds. Run it while in
# a real Zoom/Teams/Feishu/WeMeet call, muted and camera off, to see which
# signals survive. Ctrl-C to stop.
set -e
cd "$(dirname "$0")"
export SDKROOT="${SDKROOT:-/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk}"
clang -isysroot "$SDKROOT" -fobjc-arc -framework Foundation -framework CoreAudio \
  -framework CoreMediaIO -framework IOKit -framework AppKit meetingprobe.m -o /tmp/meetingprobe
exec /tmp/meetingprobe --loop
