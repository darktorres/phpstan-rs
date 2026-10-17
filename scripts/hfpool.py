"""Pools hyperfine rounds that ran the same two commands, in any order, and
compares the two: each `--export-json` file's results are split by whether the
command contains a marker, their `times` are pooled per side, and the sides are
compared with a two-sided Mann-Whitney U test in the normal approximation
(tied times take their mean rank; no tie correction).

usage: hfpool.py <marker> <hyperfine json>...
"""
import json
import math
import statistics as st
import sys

marker, paths = sys.argv[1], sys.argv[2:]
without, with_marker = [], []
for path in paths:
    with open(path) as fh:
        for result in json.load(fh)['results']:
            (with_marker if marker in result['command'] else without).extend(result['times'])
for label, times in (('without', without), ('with', with_marker)):
    print(f'{label} {marker}: {len(times)} runs, median {st.median(times):.3f} s, '
          f'mean {st.mean(times):.3f} s, sd {st.stdev(times):.3f} s')
values = sorted([(v, 0) for v in without] + [(v, 1) for v in with_marker])
ranks = {}
i = 0
while i < len(values):
    j = i
    while j + 1 < len(values) and values[j + 1][0] == values[i][0]:
        j += 1
    for k in range(i, j + 1):
        ranks[k] = (i + j) / 2 + 1
    i = j + 1
n1, n2 = len(without), len(with_marker)
u = sum(ranks[k] for k, (_, side) in enumerate(values) if side == 0) - n1 * (n1 + 1) / 2
mu, sd = n1 * n2 / 2, (n1 * n2 * (n1 + n2 + 1) / 12) ** 0.5
z = (u - mu) / sd
print(f'Mann-Whitney U {u:.1f} (the side without), z {z:.2f}, '
      f'p {math.erfc(abs(z) / 2 ** 0.5):.3f} (two-sided)')
