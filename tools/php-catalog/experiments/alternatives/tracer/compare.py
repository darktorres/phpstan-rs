#!/usr/bin/env python3
"""Compares the tracer's observations with the builtin-runs table and the type-derived bound's
vocabulary. Usage: compare.py TRACE.tsv TABLE.tsv OUT_DIR"""
import collections, json, os, sys

trace_path, table_path, out = sys.argv[1:4]
os.makedirs(out, exist_ok=True)

WRAPPER = {"stream_open", "stream_read", "stream_write", "stream_close", "stream_eof", "stream_tell",
           "stream_seek", "stream_flush", "stream_stat", "stream_lock", "stream_truncate",
           "stream_set_option", "stream_cast", "stream_metadata", "url_stat", "dir_opendir",
           "dir_readdir", "dir_closedir", "dir_rewinddir", "mkdir", "rename", "rmdir", "unlink"}
# what derived_runs (registry/effects.rs) can produce, by method name
BOUND_METHODS = {"__tostring", "getiterator", "valid", "current", "key", "next", "rewind", "count",
                 "offsetget", "offsetset", "offsetexists", "offsetunset", "jsonserialize",
                 "serialize", "unserialize", "__serialize", "__unserialize", "__sleep", "__wakeup",
                 "__debuginfo"}
BOUND_WRAPPER = {"stream_open", "stream_read", "stream_write", "stream_close", "stream_eof",
                 "stream_tell", "stream_seek", "stream_flush", "stream_stat", "stream_lock",
                 "stream_truncate", "stream_set_option", "stream_cast", "stream_metadata",
                 "url_stat", "dir_opendir", "dir_readdir", "dir_closedir", "dir_rewinddir"}

table = collections.defaultdict(set)   # fn -> {(place, kind, detail)}
probed = set()
for line in open(table_path):
    parts = line.rstrip("\n").split("\t")
    if len(parts) < 3:
        continue
    fn, place, kind = parts[0], parts[1], parts[2]
    detail = parts[3] if len(parts) > 3 else ""
    if kind == "probed":
        probed.add(fn)
    else:
        table[fn].add((place, kind, detail))

obs = collections.defaultdict(lambda: {"count": 0, "scripts": set()})
bad = 0
for line in open(trace_path, errors="replace"):
    parts = line.rstrip("\n").split("\t")
    if len(parts) != 8:
        bad += 1
        continue
    fn, ufn, ucls, this_rel, clo_rel, arg0_rel, count, script = parts
    fn_l = fn.lower()
    m = ufn.lower()
    if clo_rel not in ("-", ""):
        key = (fn_l, clo_rel, "calls", "")
    elif this_rel not in ("-", ""):
        key = (fn_l, this_rel, "method", m)
    elif m in WRAPPER and ucls != "-":
        key = (fn_l, "?", "wrapper", m)
    elif arg0_rel not in ("-", ""):
        key = (fn_l, arg0_rel, "receives-argument", m)
    elif ufn == "{closure}" or ucls == "-":
        key = (fn_l, "?", "user-function", m)
    else:
        key = (fn_l, "?", "other-method", m)
    o = obs[key]
    o["count"] += int(count)
    o["scripts"].add(script.replace("/home/torres/.cache/php-alt-exp/exp3/php-src-tests/", ""))

def in_table(fn, place, kind, detail):
    rows = table.get(fn, set())
    if kind == "wrapper":
        return any(k == "wrapper" and d == detail for (_, k, d) in rows)
    if kind == "calls":
        return (place, "calls", "") in rows
    if kind == "method":
        return (place, "method", detail) in rows
    return None  # not expressible as a table row

summary = collections.Counter()
new_rows, bound_holes = [], []
for (fn, place, kind, detail), o in sorted(obs.items()):
    t = in_table(fn, place, kind, detail)
    status = "table" if t else ("missing" if t is False else "not-a-row")
    summary[(kind, status)] += 1
    rec = {"fn": fn, "place": place, "kind": kind, "detail": detail, "count": o["count"],
           "scripts": len(o["scripts"]), "example": sorted(o["scripts"])[0], "in_table": status,
           "probed": fn in probed}
    if status == "missing":
        new_rows.append(rec)
    if (kind == "method" and detail not in BOUND_METHODS) or (kind == "wrapper" and detail not in BOUND_WRAPPER):
        bound_holes.append(rec)

json.dump({"bad_lines": bad, "observed_keys": len(obs),
           "summary": {f"{k}/{s}": n for (k, s), n in sorted(summary.items())}},
          open(os.path.join(out, "summary.json"), "w"), indent=1)
for name, rows in (("missing-from-table", new_rows), ("outside-bound-vocabulary", bound_holes)):
    with open(os.path.join(out, name + ".tsv"), "w") as f:
        f.write("fn\tplace\tkind\tdetail\tcount\tscripts\tprobed\texample\n")
        for r in rows:
            f.write(f"{r['fn']}\t{r['place']}\t{r['kind']}\t{r['detail']}\t{r['count']}\t{r['scripts']}\t{r['probed']}\t{r['example']}\n")
with open(os.path.join(out, "all-observed.tsv"), "w") as f:
    f.write("fn\tplace\tkind\tdetail\tcount\tscripts\tin_table\texample\n")
    for (fn, place, kind, detail), o in sorted(obs.items()):
        t = in_table(fn, place, kind, detail)
        st = "table" if t else ("missing" if t is False else "not-a-row")
        f.write(f"{fn}\t{place}\t{kind}\t{detail}\t{o['count']}\t{len(o['scripts'])}\t{st}\t{sorted(o['scripts'])[0]}\n")
print(json.dumps({"bad_lines": bad, "observed_keys": len(obs),
                  "summary": {f"{k}/{s}": n for (k, s), n in sorted(summary.items())},
                  "missing": len(new_rows), "outside_bound": len(bound_holes)}, indent=1))
