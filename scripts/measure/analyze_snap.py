#!/usr/bin/env python3
"""Per-interval CPU accounting from SNAP lines (process CPU and runnable wait at each mark).

usage: analyze_snap.py <vcpus> <run.err>...

For each begin/end pair: dur; ours = the process's CPU time over the interval (exact);
idle = vCPUs minus ours, so it includes CPU used by other processes.
Runnable wait is the summed run-queue delay of the process's tasks over the interval.
The medians across the given runs are printed per interval label.
"""
import collections
import statistics
import sys

vcpus = int(sys.argv[1])
files = sys.argv[2:]


def parse(path):
    snaps = []
    with open(path, errors="replace") as f:
        for line in f:
            p = line.split()
            if len(p) < 16 or p[0] != "SNAP":
                continue
            snaps.append({
                "label": p[1],
                "t": int(p[2]),
                "cpu": int(p[3]),
                "wait": int(p[4]),
                "jiffies": [int(x) for x in p[5:15]],
                "thread": p[15],
            })
    return snaps


results = collections.defaultdict(list)  # label -> list of dicts per run
for path in files:
    snaps = parse(path)
    if not snaps:
        continue
    origin = next((s["t"] for s in snaps if s["label"] == "command-start"), snaps[0]["t"])
    open_marks = collections.defaultdict(list)
    intervals = []  # (label, begin mark, end mark) in end order
    for s in snaps:
        label = s["label"]
        if label.endswith("-begin"):
            open_marks[(label[:-6], s["thread"])].append(s)
        elif label.endswith("-end"):
            key = (label[:-4], s["thread"])
            if not open_marks[key]:
                continue
            intervals.append((key[0], open_marks[key].pop(), s))
    # One registry pass per run is the long one on the command thread; keep its steps only.
    registry = [iv for iv in intervals if iv[0] == "registry"]
    main = max(registry, key=lambda iv: iv[2]["t"] - iv[1]["t"]) if registry else None
    for name, b, e in intervals:
        outer = name in ("listing-wait", "listing-walk", "vendor-get")
        if main and name != "registry" and not outer and not (main[1]["t"] <= b["t"] <= main[2]["t"]):
            continue
        if name == "registry" and (b, e) != (main[1], main[2]):
            continue
        dur = (e["t"] - b["t"]) / 1e9
        if dur <= 0:
            continue
        ours = e["cpu"] - b["cpu"]
        idle_cs = max(0.0, vcpus * dur - ours / 1e9)
        results[name].append({
            "start": (b["t"] - origin) / 1e9,
            "dur": dur,
            "ours": ours / 1e9 / dur,
            "idle": idle_cs / dur,
            "idle_cs": idle_cs,
            "wait_cs": (e["wait"] - b["wait"]) / 1e9,
        })
    # The whole run: command-start to the last mark.
    start = next((s for s in snaps if s["label"] == "command-start"), snaps[0])
    last = snaps[-1]
    span = (last["t"] - start["t"]) / 1e9
    ours = last["cpu"] - start["cpu"]
    idle = max(0.0, vcpus * span - ours / 1e9)
    results["whole-run"].append({
        "start": 0.0,
        "dur": span,
        "ours": ours / 1e9 / span,
        "idle": idle / span,
        "idle_cs": idle,
        "wait_cs": (last["wait"] - start["wait"]) / 1e9,
    })

print(f"{'interval':<40}{'n':>3}{'start':>8}{'dur':>8}{'ours':>7}{'':>8}{'idle':>7}{'idle c-s':>10}{'wait c-s':>10}")
for label in sorted(results, key=lambda k: min(r["start"] for r in results[k])):
    rows = results[label]
    med = lambda k: statistics.median(r[k] for r in rows)
    print(f"{label:<40}{len(rows):>3}{med('start'):8.3f}{med('dur'):8.3f}{med('ours'):7.2f}"
          f"{'':>8}{med('idle'):7.2f}{med('idle_cs'):10.3f}{med('wait_cs'):10.3f}")
print(f"\nvcpus={vcpus}; medians over the runs that contain each interval")
