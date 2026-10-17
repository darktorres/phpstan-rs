"""Callgrind Ir of one run directory: the total over its per-thread files, and
the functions with the largest self and inclusive costs. With a second run,
the difference by function name (line numbers move between builds, so
functions are matched by name).

Each file's `summary:` line is that thread's total; the sum of its self costs
must equal it, which checks the parse.

usage: cgfn.py <run dir> [<run dir> [n]]
"""
import glob
import os
import sys
from collections import defaultdict


def parse(path):
    names = {}
    self_c = defaultdict(int)
    incl_c = defaultdict(int)
    total = None
    events = None
    fn = None
    in_call = False

    def resolve(text):
        # `(12) name` defines id 12; `(12)` refers to it.
        if text.startswith('('):
            ident, _, rest = text.partition(')')
            key = ident + ')'
            if rest.strip():
                names[key] = rest.strip()
            return names.get(key, key)
        return text

    with open(path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            if line.startswith('fn='):
                fn = resolve(line[3:].strip())
            elif line.startswith('cfn='):
                resolve(line[4:].strip())
            elif line.startswith('calls='):
                in_call = True
            elif line.startswith('events:'):
                events = line.split(':', 1)[1].split()
            elif line.startswith('summary:'):
                total = int(line.split(':', 1)[1].split()[0])
            elif line[:1].isdigit() or line[:1] in '+-*':
                cost = int(line.split()[-1])
                if in_call:
                    in_call = False
                    incl_c[fn] += cost
                else:
                    self_c[fn] += cost
                    incl_c[fn] += cost
    if events != ['Ir']:
        raise SystemExit(f'{path}: events {events}, expected Ir')
    if total is None or sum(self_c.values()) != total:
        raise SystemExit(f'{path}: self costs {sum(self_c.values())} != summary {total}')
    return total, self_c, incl_c


def run_totals(d):
    files = sorted(glob.glob(os.path.join(d, 'cg.out-*')))
    if not files:
        raise SystemExit(f'{d}: no cg.out-* files')
    total = 0
    self_c = defaultdict(int)
    incl_c = defaultdict(int)
    per_thread = []
    for f in files:
        t, s, i = parse(f)
        total += t
        per_thread.append((t, os.path.basename(f)))
        for k, v in s.items():
            self_c[k] += v
        for k, v in i.items():
            incl_c[k] += v
    return total, per_thread, self_c, incl_c


def top(label, d, n):
    print(f'\n-- {label}, top {n}')
    for k in sorted(d, key=lambda k: -d[k])[:n]:
        print(f'{d[k]:16,}  {k}')


def main():
    a = sys.argv[1]
    b = sys.argv[2] if len(sys.argv) > 2 else None
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 40
    ta, pta, sa, ia = run_totals(a)
    print(f'{a}: Ir {ta:,}')
    for t, name in pta:
        print(f'  {name}: {t:,}')
    if b is None:
        top('self', sa, n)
        top('inclusive', ia, n)
        return
    tb, ptb, sb, ib = run_totals(b)
    print(f'{b}: Ir {tb:,}')
    for t, name in ptb:
        print(f'  {name}: {t:,}')
    print(f'\ndifference {a} minus {b}: {ta - tb:+,} ({(ta - tb) / tb * 100:+.4f}% of {b})')
    for label, da, db in (('self', sa, sb), ('inclusive', ia, ib)):
        d = {k: da.get(k, 0) - db.get(k, 0) for k in set(da) | set(db)}
        print(f'\n-- {label}, largest differences ({a} minus {b}):')
        print(f'{"delta":>16} {a:>16} {b:>16}  function')
        for k in sorted(d, key=lambda k: -abs(d[k]))[:n]:
            print(f'{d[k]:+16,} {da.get(k, 0):16,} {db.get(k, 0):16,}  {k}')


if __name__ == '__main__':
    main()
