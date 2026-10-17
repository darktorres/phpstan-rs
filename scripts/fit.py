"""Scaling table and fit from a hyperfine --export-json over PHPSTAN_RS_THREADS.

usage: fit.py <hyperfine json>...

Per thread count: wall mean (sigma), median, and CPU (user + system means).
Then least squares of wall = serial + parallel / threads, on the means and on
the medians, with each point's fit minus measured, in % of measured. The form
assumes the CPU time stays the same as threads are added; the CPU column shows
whether it does.
"""
import json
import re
import sys


def fit(points):
    xs = [1 / t for t, _ in points]
    ys = [w for _, w in points]
    n = len(xs)
    mx, my = sum(xs) / n, sum(ys) / n
    b = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sum((x - mx) ** 2 for x in xs)
    return my - b * mx, b


for path in sys.argv[1:]:
    rows = []
    for r in json.load(open(path))['results']:
        threads = int(re.search(r'PHPSTAN_RS_THREADS=(\d+)', r['command']).group(1))
        rows.append((threads, r['mean'] * 1000, r['stddev'] * 1000, r['median'] * 1000,
                     r['user'], r['system']))
    rows.sort()
    print(path)
    print(f'{"threads":>7} {"mean":>7} {"sigma":>6} {"median":>7} {"CPU s":>6} {"user":>6} {"sys":>5}')
    for t, mean, sd, med, user, system in rows:
        print(f'{t:7} {mean:7,.0f} {sd:6,.0f} {med:7,.0f} {user + system:6.2f} {user:6.2f} {system:5.2f}')
    for label, col in (('means', 1), ('medians', 3)):
        points = [(r[0], r[col]) for r in rows]
        serial, parallel = fit(points)
        res = ', '.join(f'{((serial + parallel / t) - w) / w * 100:+.1f}%' for t, w in points)
        print(f'fit on the {label}: serial {serial:,.0f} ms, parallel {parallel:,.0f} ms; '
              f'fit minus measured {res}')
