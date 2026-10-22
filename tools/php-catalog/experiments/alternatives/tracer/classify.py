"""Classifies one observed key against the builtin-runs table.

A key is (fn, place, kind, detail), as compare.py builds it from the tracer's lines:

- calls: a closure that is an argument of the builtin, at place N;
- method: a user method run on an object at place N, N[] or this;
- wrapper: a stream-wrapper method run by a user class object;
- receives-argument: a user function whose first argument is an object of the builtin's
  arguments (a callback given an element, or a closure registered earlier that receives one);
- user-function: a user function run by the builtin, with no object relation;
- other-method: a user method on an object that is in none of the builtin's argument slots.

Status values:

- table: the table has the row.
- missing: the key has a row's form, and the table lacks the row.
- calls-kind: a user callable run by a builtin the table has a calls row for. The trace cannot
  place a named callable, so the row's place is not checked.
- not-a-row/<class>: the key has no row form. The class says which mechanism the table cannot
  name: a registered-callback, a lazy-initializer (H3), a receives-argument, a generator-resume,
  a callable-unplaced, or a reached-otherwise (an object held in state, nested in an argument,
  or freed by the collector).
"""


def load_table(path):
    """Returns (table, probed): fn -> {(place, kind, detail)}, and the probed function names."""
    table = {}
    probed = set()
    with open(path) as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 3:
                continue
            fn, place, kind = parts[0], parts[1], parts[2]
            detail = parts[3] if len(parts) > 3 else ""
            if kind == "probed":
                probed.add(fn)
            else:
                table.setdefault(fn, set()).add((place, kind, detail))
    return table, probed


def status(key, table):
    fn, place, kind, detail = key
    rows = table.get(fn, set())
    if kind == "wrapper":
        expressible = True
        present = any(k == "wrapper" and d == detail for (_, k, d) in rows)
    elif kind == "calls":
        expressible = True
        present = (place, "calls", "") in rows
    elif kind == "method":
        expressible = True
        present = (place, "method", detail) in rows
    else:
        expressible = False
        present = False
    if expressible:
        return "table" if present else "missing"
    if kind in ("user-function", "receives-argument") and any(k == "calls" for (_, k, _) in rows):
        return "calls-kind"
    if kind == "receives-argument":
        if fn == "reflectionclass::initializelazyobject":
            return "not-a-row/lazy-initializer"
        if detail == "{closure}":
            return "not-a-row/registered-callback"
        return "not-a-row/receives-argument"
    if kind == "user-function":
        if fn.startswith("generator::"):
            return "not-a-row/generator-resume"
        return "not-a-row/callable-unplaced"
    return "not-a-row/reached-otherwise"
