#!/bin/bash
# Run rssi-scan and capture its CSV into a dated log, for the long Doze observation.
#
# Usage: ./rssi-log.sh [SECONDS]   (default 28800 = 8h; 0 = run until Ctrl-C)
#
# The point of this run is to measure how continuous the phone's advertising is
# once the screen is off and the phone is on battery. Two things would corrupt
# that measurement, and both are handled here rather than left to the operator:
#
#   * the Mac going to sleep, which stops CoreBluetooth and writes a multi-hour
#     gap into the CSV that looks exactly like the phone going silent
#   * a stale rssi-scan binary from before a source change
#
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
secs="${1:-28800}"
outdir="$here/logs"
stamp="$(date +%Y%m%d-%H%M%S)"
csv="$outdir/rssi-$stamp.csv"
err="$outdir/rssi-$stamp.log"

mkdir -p "$outdir"

# Rebuild whenever the source is newer than the binary. Checking only for
# existence silently runs yesterday's build after a source fix.
if [ ! -x "$here/rssi-scan" ] || [ "$here/rssi-scan.swift" -nt "$here/rssi-scan" ]; then
  echo "building rssi-scan..." >&2
  swiftc -O -o "$here/rssi-scan" "$here/rssi-scan.swift" -framework CoreBluetooth
fi

# macOS ships bash 3.2, where an empty array expanded under `set -u` is an
# unbound variable error rather than an empty list. Passing the arguments
# positionally keeps `./rssi-log.sh 0` from dying on that.
set --
if [ "$secs" != "0" ]; then
  set -- --duration "$secs"
fi

# caffeinate -i blocks idle sleep for as long as the scan runs. Without it an
# overnight run ends whenever the Mac decides to nap.
caffeinate_cmd=""
if command -v caffeinate >/dev/null 2>&1; then
  caffeinate_cmd="caffeinate -i"
else
  echo "WARNING: caffeinate not found; the Mac may sleep and truncate the run" >&2
fi

echo "csv -> $csv" >&2
echo "log -> $err" >&2
if [ "$secs" != "0" ]; then
  echo "duration -> ${secs}s ($(echo "scale=1; $secs/3600" | bc)h), idle sleep blocked" >&2
else
  echo "duration -> until Ctrl-C, idle sleep blocked" >&2
fi

status=0
# shellcheck disable=SC2086
$caffeinate_cmd "$here/rssi-scan" "$@" >"$csv" 2>"$err" || status=$?

samples="$(wc -l < "$csv" | tr -d ' ')"
echo "scan exited ${status}, ${samples} samples" >&2
echo "analyze with: $here/analyze-rssi.py $csv" >&2

# Surface the scanner's exit status instead of swallowing it, so an unattended
# run that died early is not mistaken for a clean one.
exit "$status"
