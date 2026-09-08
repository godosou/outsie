#!/usr/bin/env python3
"""Summarize an rssi-scan CSV: sampling cadence, worst gap, RSSI spread.

The question this answers: after the phone's screen goes off, did the OS keep
advertising or silently cut it? A long max gap is the tell.

Usage: ./analyze-rssi.py logs/rssi-YYYYmmdd-HHMMSS.csv [--gap SECONDS]
"""
import sys
from datetime import datetime, timezone


def pct(sorted_vals, p):
    if not sorted_vals:
        return float("nan")
    i = min(len(sorted_vals) - 1, int(round((p / 100.0) * (len(sorted_vals) - 1))))
    return sorted_vals[i]


def hms(ms):
    s = ms / 1000.0
    return f"{int(s // 3600):02d}:{int(s // 60) % 60:02d}:{s % 60:05.2f}"


def main():
    args = [a for a in sys.argv[1:]]
    gap_threshold = 10.0
    if "--gap" in args:
        i = args.index("--gap")
        gap_threshold = float(args[i + 1])
        del args[i:i + 2]
    if not args:
        print(__doc__)
        return 64

    rows = []
    bad = 0
    with open(args[0]) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split(",")
            if len(parts) != 3:
                bad += 1
                continue
            try:
                rows.append((int(parts[0]), int(parts[1]), parts[2]))
            except ValueError:
                bad += 1

    if not rows:
        print(f"no samples ({bad} unparseable lines)")
        return 1

    rows.sort(key=lambda r: r[0])
    t0, t1 = rows[0][0], rows[-1][0]
    span = t1 - t0
    deltas = sorted(b[0] - a[0] for a, b in zip(rows, rows[1:]))
    rssis = sorted(r[1] for r in rows)
    peers = {}
    for _, _, pid in rows:
        peers[pid] = peers.get(pid, 0) + 1

    def ts(ms):
        return datetime.fromtimestamp(ms / 1000, timezone.utc).astimezone().isoformat(
            timespec="seconds")

    print(f"file           {args[0]}")
    print(f"samples        {len(rows)}" + (f"  ({bad} bad lines)" if bad else ""))
    print(f"window         {ts(t0)}  ->  {ts(t1)}")
    print(f"duration       {hms(span)}")
    print(f"rate           {len(rows) / (span / 1000.0):.2f} samples/s"
          if span else "rate           n/a")
    print()
    if deltas:
        print("sampling interval (ms)")
        for p in (50, 95, 99):
            print(f"  p{p:<3}         {pct(deltas, p):.0f}")
        print(f"  max          {deltas[-1]:.0f}  ({hms(deltas[-1])})")
        print()

        gaps = [(a[0], b[0] - a[0]) for a, b in zip(rows, rows[1:])
                if (b[0] - a[0]) > gap_threshold * 1000]
        print(f"gaps > {gap_threshold:g}s    {len(gaps)}")
        for start, d in sorted(gaps, key=lambda g: -g[1])[:10]:
            print(f"  {hms(d)}  starting {ts(start)}")
        if gaps:
            total = sum(d for _, d in gaps)
            print(f"  dark time    {hms(total)}  "
                  f"({100.0 * total / span:.1f}% of window)")
        print()
    else:
        print("sampling interval   n/a (need 2+ samples)")
        print()

    print("rssi (dBm)")
    print(f"  min/max      {rssis[0]} / {rssis[-1]}")
    for p in (5, 50, 95):
        print(f"  p{p:<3}         {pct(rssis, p)}")
    print(f"  mean         {sum(rssis) / len(rssis):.1f}")
    hist = {}
    for v in rssis:
        hist[(v // 10) * 10] = hist.get((v // 10) * 10, 0) + 1
    for bucket in sorted(hist):
        n = hist[bucket]
        print(f"  [{bucket},{bucket + 10})  {n:6d}  "
              + "#" * max(1, int(40 * n / len(rssis))))
    print()

    print("peripherals")
    for pid, n in sorted(peers.items(), key=lambda kv: -kv[1]):
        print(f"  {pid}     {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
