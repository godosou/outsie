#!/bin/bash
# Run rssi-scan and tee its CSV into a dated log, for the long Doze observation.
# Usage: ./rssi-log.sh [SECONDS]   (default 28800 = 8h; 0 = run until Ctrl-C)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
secs="${1:-28800}"
outdir="$here/logs"
stamp="$(date +%Y%m%d-%H%M%S)"
csv="$outdir/rssi-$stamp.csv"
err="$outdir/rssi-$stamp.log"

mkdir -p "$outdir"

if [ ! -x "$here/rssi-scan" ]; then
  echo "building rssi-scan..." >&2
  swiftc -O -o "$here/rssi-scan" "$here/rssi-scan.swift" -framework CoreBluetooth
fi

args=()
[ "$secs" != "0" ] && args=(--duration "$secs")

echo "csv -> $csv" >&2
echo "log -> $err" >&2
"$here/rssi-scan" "${args[@]}" >"$csv" 2>"$err" || echo "scan exited $?" >&2
echo "done. analyze with: $here/analyze-rssi.py $csv" >&2
