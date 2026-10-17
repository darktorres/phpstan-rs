"""Wall time of two gate binaries on Piwigo, run alternately: the pairs run
A B, B A, A B, ..., so drift in the machine's load falls on both sides alike.
Every run's output is checked against the oracle's sha256. The pairs are
compared by the difference B - A within each pair, with a Wilcoxon
signed-rank test in the normal approximation (zero differences dropped, tied
magnitudes take their mean rank, no tie correction). Each run's peak RSS and
CPU time (user + system, from wait4) are compared the same way.

usage: abab.py <out dir> <pairs> <off|warm> <name A> <name B>

A name is a directory of .corpus-runs/gate/bin with a gate/phpstan-rs in it.
Warm mode gives each binary a cache dir of its own in <out dir>, filled by
two unrecorded runs of each, which also warm the off mode's stub reads.
Run it under nice -n 19; the runs inherit it.
"""
import hashlib
import json
import math
import os
import statistics as st
import subprocess
import sys
import time

BIN = '/home/torres/projects/phpstan-rs/.corpus-runs/gate/bin'
PIW = '/home/torres/piwigo17-frozen'
CHECK = ['check-project', '--config', 'phpstan-rs.toml', '--format', 'json']
ORACLE = '8ff144c3fee775166369b993a58814e0f247845fbf850f94d0c41b57924488b1'


def load():
    with open('/proc/loadavg') as fh:
        return ' '.join(fh.read().split()[:3])


def run(out, mode, name):
    env = dict(os.environ)
    if mode == 'off':
        env['PHPSTAN_RS_NO_FILE_CACHE'] = '1'
        env['PHPSTAN_RS_NO_STUB_CACHE'] = '1'
    else:
        env['PHPSTAN_RS_CACHE_DIR'] = os.path.join(out, 'cache-' + name)
    path = os.path.join(out, 'out-' + name + '.json')
    with open(path, 'wb') as stdout:
        start = time.perf_counter()
        child = subprocess.Popen([os.path.join(BIN, name, 'gate', 'phpstan-rs')] + CHECK,
                                 cwd=PIW, env=env, stdout=stdout,
                                 stderr=subprocess.DEVNULL)
        _, wait_status, usage = os.wait4(child.pid, 0)
        wall = time.perf_counter() - start
    status = child.returncode = os.waitstatus_to_exitcode(wait_status)
    with open(path, 'rb') as fh:
        digest = hashlib.sha256(fh.read()).hexdigest()
    if status != 0 or digest != ORACLE:
        sys.exit(f'{name}: exit {status}, output sha256 {digest}')
    return wall, usage.ru_maxrss, usage.ru_utime + usage.ru_stime


def wilcoxon(diffs):
    nonzero = [d for d in diffs if d != 0]
    ranked = sorted(nonzero, key=abs)
    ranks = [0.0] * len(ranked)
    i = 0
    while i < len(ranked):
        j = i
        while j + 1 < len(ranked) and abs(ranked[j + 1]) == abs(ranked[i]):
            j += 1
        for k in range(i, j + 1):
            ranks[k] = (i + j) / 2 + 1
        i = j + 1
    w_plus = sum(r for r, d in zip(ranks, ranked) if d > 0)
    n = len(ranked)
    mean = n * (n + 1) / 4
    sd = math.sqrt(n * (n + 1) * (2 * n + 1) / 24)
    z = (w_plus - mean) / sd if sd else 0.0
    p = math.erfc(abs(z) / math.sqrt(2))
    return w_plus, z, p


def main():
    out, pairs, mode, a, b = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
    os.makedirs(out, exist_ok=True)
    log = open(os.path.join(out, 'run.log'), 'a')
    log.write(f'{time.strftime("%Y-%m-%dT%H:%M:%S")} load {load()} start {mode} {a} {b} {pairs} pairs\n')
    for name in (a, b, a, b):
        run(out, mode, name)
    times, rss, cpu = {a: [], b: []}, {a: [], b: []}, {a: [], b: []}
    for i in range(pairs):
        order = (a, b) if i % 2 == 0 else (b, a)
        for name in order:
            wall, maxrss, used = run(out, mode, name)
            times[name].append(wall)
            rss[name].append(maxrss)
            cpu[name].append(used)
    log.write(f'{time.strftime("%Y-%m-%dT%H:%M:%S")} load {load()} done\n')
    log.close()
    with open(os.path.join(out, 'times.json'), 'w') as fh:
        json.dump({'mode': mode, 'a': a, 'b': b, 'times': times, 'maxrss_kib': rss, 'cpu': cpu}, fh)
    diffs = [tb - ta for ta, tb in zip(times[a], times[b])]
    for name in (a, b):
        t = times[name]
        print(f'{name}: {len(t)} runs, median {st.median(t):.3f} s, mean {st.mean(t):.3f} s, sd {st.stdev(t):.3f} s; '
              f'peak RSS median {st.median(rss[name]) / 1024:.1f} MiB; CPU median {st.median(cpu[name]):.2f} s')
    w_plus, z, p = wilcoxon(diffs)
    print(f'{b} - {a}, paired: median {st.median(diffs) * 1000:+.1f} ms, mean {st.mean(diffs) * 1000:+.1f} ms, '
          f'{sum(d > 0 for d in diffs)} of {len(diffs)} pairs slower; Wilcoxon W+ {w_plus:.1f}, z {z:.2f}, p {p:.3f}')
    for label, values, scale, unit in (('peak RSS', rss, 1 / 1024, 'MiB'), ('CPU', cpu, 1, 's')):
        d = [(vb - va) * scale for va, vb in zip(values[a], values[b])]
        w_plus, z, p = wilcoxon(d)
        print(f'{label}, paired: median {st.median(d):+.2f} {unit}, mean {st.mean(d):+.2f} {unit}; '
              f'Wilcoxon W+ {w_plus:.1f}, z {z:.2f}, p {p:.3f}')


if __name__ == '__main__':
    main()
