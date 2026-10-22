#!/usr/bin/env python3
"""Replays the tracer's observed keys against the type-derived bound.

Usage: replay.py ALL_OBSERVED.tsv TABLE.tsv DERIVED.tsv OUT_DIR

DERIVED.tsv is written by the ignored test writes_the_derived_bound (crates/phpstan-rs, registry/
effects/derived_bound_dump.rs). For every observed key it answers two questions:

- D, the type-derived bound alone: does derived_runs give the run? A method run needs a parameter
  at that place (elements for N[]) whose type gives it; a wrapper run needs the method on some
  parameter; a method of an object in no argument slot needs the method on some parameter.
  derived_runs gives no callable run, so a callback is never covered by D.
- U, the shipped authority: the table, or D, or a parameter typed as a callable (its argument's
  own runs, which takes_callable gives), or, for $this, a method the builtin's class declares
  (the override rule in compute_builtin_base), or a method run on an argument at a callable
  parameter (the argument's own runs, which the callable's argument rule gives), or the
  fail-closed fallback of a method whose object runs what it was made with (build.rs's HOLDING:
  the checker treats such a call as running anything).

Not modelled: the checker's rules outside the table and bound (generator bodies, objects
released by a builtin, state a builtin keeps between calls). A miss here is a key the table and
bound do not name, not a proof that the checker misses it.

Holes are named as in PHP_SEMANTICS_CATALOG_ALTERNATIVES.md: H1 rename on a stream wrapper,
H2 array_column reading __get, H3 a lazy object's initializer.
"""
import collections, json, os, sys

import classify

all_path, table_path, derived_path, out = sys.argv[1:5]
os.makedirs(out, exist_ok=True)

table, _ = classify.load_table(table_path)
# A method whose object runs what it was made with is unknown to the checker (build.rs's HOLDING):
# its base is not known, so the checker treats the call as running anything.
holding = set()
with open(table_path) as f:
    for line in f:
        parts = line.rstrip("\n").split("\t")
        if len(parts) >= 3 and parts[1].startswith("new:") and parts[2] != "wrapper":
            holding.add(parts[0])

methods = collections.defaultdict(set)    # class -> declared method names
params = collections.defaultdict(list)    # key -> [(at, name, takes_callable)]
runs = collections.defaultdict(set)       # key -> {(at, elements, kind, name)}
with open(derived_path) as f:
    for line in f:
        parts = line.rstrip("\n").split("\t")
        if parts[0] == "M":
            methods[parts[1]].add(parts[2])
        elif parts[0] == "P":
            params[parts[1]].append((int(parts[2]), parts[3], parts[4] == "1"))
        elif parts[0] == "R":
            runs[parts[1]].add((int(parts[2]), parts[3] == "1", parts[4], parts[5]))


def parse_place(place):
    """(position, elements) for "N" and "N[]"; (None, None) for "this" and "?"."""
    if place.endswith("[]") and place[:-2].isdigit():
        return int(place[:-2]), True
    if place.isdigit():
        return int(place), False
    return None, None


def derived_covers(fn, place, kind, detail):
    rs = runs.get(fn, set())
    if kind == "method":
        at, elements = parse_place(place)
        return at is not None and (at, elements, "method", detail) in rs
    if kind == "wrapper":
        return any(k == "wrapper" and n == detail for (_, _, k, n) in rs)
    if kind == "other-method":
        return any(k == "method" and n == detail for (_, _, k, n) in rs)
    return False


def union_covers(fn, place, kind, detail, status):
    if status in ("table", "calls-kind"):
        return True
    if derived_covers(fn, place, kind, detail):
        return True
    if kind == "method" and place == "this":
        cls = fn.rsplit("::", 1)[0] if "::" in fn else ""
        return detail in methods.get(cls, set())
    at, _ = parse_place(place)
    takes = [(a, cb) for (a, _, cb) in params.get(fn, [])]
    if kind == "calls":
        return any(cb and a == at for (a, cb) in takes)
    if kind == "method":
        return any(cb and a == at for (a, cb) in takes)
    if kind in ("receives-argument", "user-function"):
        return any(cb for (_, cb) in takes)
    return False


def hole(fn, kind, detail, status):
    if kind == "wrapper" and detail == "rename":
        return "H1"
    if fn == "array_column" and kind == "method" and detail == "__get":
        return "H2"
    if status == "not-a-row/lazy-initializer":
        return "H3"
    return ""


rows = []
with open(all_path) as f:
    header = f.readline().rstrip("\n").split("\t")
    assert header[:5] == ["fn", "place", "kind", "detail", "count"], header
    for line in f:
        fn, place, kind, detail, count, _scripts, _old, example = line.rstrip("\n").split("\t")
        status = classify.status((fn, place, kind, detail), table)
        d = derived_covers(fn, place, kind, detail)
        fallback = fn in holding
        u = union_covers(fn, place, kind, detail, status) or fallback
        rows.append({"fn": fn, "place": place, "kind": kind, "detail": detail,
                     "count": int(count), "status": status, "derived": d, "union": u,
                     "fallback": fallback,
                     "hole": hole(fn, kind, detail, status), "example": example})


def group(keyfn):
    stats = collections.defaultdict(lambda: {"keys": 0, "calls": 0, "derived": 0, "union": 0})
    for r in rows:
        s = stats[keyfn(r)]
        s["keys"] += 1
        s["calls"] += r["count"]
        s["derived"] += r["derived"]
        s["union"] += r["union"]
    return {k: stats[k] for k in sorted(stats)}


def misses(rs):
    return [r for r in rs if not r["union"]]


summary = {
    "keys": len(rows),
    "calls": sum(r["count"] for r in rows),
    "derived_covered_keys": sum(r["derived"] for r in rows),
    "union_covered_keys": sum(r["union"] for r in rows),
    "fallback_covered_keys": sum(r["fallback"] for r in rows),
    "by_status": group(lambda r: r["status"]),
    "by_hole": group(lambda r: r["hole"] or "none"),
    "union_misses_outside_holes": len([r for r in misses(rows) if not r["hole"]]),
    "union_misses_outside_holes_calls": sum(r["count"] for r in misses(rows) if not r["hole"]),
    "union_misses_by_status": dict(collections.Counter(
        r["status"] for r in misses(rows) if not r["hole"])),
}
with open(os.path.join(out, "summary.json"), "w") as f:
    json.dump(summary, f, indent=1)
    f.write("\n")

with open(os.path.join(out, "union-misses.tsv"), "w") as f:
    f.write("fn\tplace\tkind\tdetail\tcount\tstatus\thole\tderived\texample\n")
    for r in sorted(misses(rows), key=lambda r: (-r["count"], r["fn"], r["detail"])):
        f.write(f"{r['fn']}\t{r['place']}\t{r['kind']}\t{r['detail']}\t{r['count']}\t"
                f"{r['status']}\t{r['hole']}\t{int(r['derived'])}\t{r['example']}\n")

print(json.dumps(summary, indent=1))
