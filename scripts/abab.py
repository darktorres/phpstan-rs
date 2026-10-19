"""Wall time of two binaries, run alternately: the pairs run A B, B A, A B, ..., so
drift in the machine's load falls on both sides alike. Every run's output is checked
against an oracle sha256 (the Piwigo oracle by default), or, with --oracle none, the
outputs of A and B must match each other. The pairs are compared by the difference
B - A within each pair, with a Wilcoxon signed-rank test in the normal approximation
(zero differences dropped, tied magnitudes take their mean rank, no tie correction),
and a bootstrap 95% interval of the median difference. Each run's peak RSS and CPU
time (user + system, from wait4) are compared the same way.

usage: abab.py <out dir> <pairs> <off|warm> <name or path A> <name or path B>
           [--cwd DIR] [--argv "check-project ..."] [--oracle SHA|none]
           [--gate FRACTION] [--machine]

A name is a directory of .corpus-runs/gate/bin with a gate/phpstan-rs in it; a path
(containing a slash) is a binary used as it is. Warm mode gives each binary a cache
dir of its own in <out dir>, filled by two unrecorded runs of each, which also warm
the off mode's stub reads. Run it under nice -n 19; the runs inherit it.

--gate waits before each run until the machine's busy fraction over the second before
it is at most FRACTION (and gives up after two minutes, recording the wait).
--machine records each run's CPU of the other processes (their utime + stime, in
ticks, from /proc) and prints the ones that used the most.
Every invocation writes manifest.json to <out dir>: the commit, the binaries, the
settings, the kernel and the load.

Linux only: it reads /proc.
"""
import argparse
import hashlib
import json
import math
import os
import random
import shlex
import statistics as st
import subprocess
import sys
import time

BIN = '/home/torres/projects/phpstan-rs/.corpus-runs/gate/bin'
PIW = '/home/torres/piwigo17-frozen'
CHECK = ['check-project', '--config', 'phpstan-rs.toml', '--format', 'json']
ORACLE = '8ff144c3fee775166369b993a58814e0f247845fbf850f94d0c41b57924488b1'
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def load():
    with open('/proc/loadavg') as fh:
        return ' '.join(fh.read().split()[:3])


def binary_path(name):
    return name if '/' in name else os.path.join(BIN, name, 'gate', 'phpstan-rs')


def label_of(name):
    return name.replace('/', '_')


def busy_fraction(seconds=1.0):
    def stat():
        with open('/proc/stat') as fh:
            v = [int(x) for x in fh.readline().split()[1:]]
        return v[3] + v[4], sum(v)
    idle0, total0 = stat()
    time.sleep(seconds)
    idle1, total1 = stat()
    return 1 - (idle1 - idle0) / (total1 - total0) if total1 > total0 else 0.0


def process_ticks():
    """pid -> (comm, utime + stime in ticks) for every process."""
    out = {}
    for pid in os.listdir('/proc'):
        if not pid.isdigit():
            continue
        try:
            with open(f'/proc/{pid}/stat') as fh:
                text = fh.read()
        except OSError:
            continue
        comm = text[text.index('(') + 1:text.rindex(')')]
        fields = text[text.rindex(')') + 2:].split()
        out[int(pid)] = (comm, int(fields[11]) + int(fields[12]))
    return out


def run(out, mode, name, args):
    path_bin = binary_path(name)
    label = label_of(name)
    waited = 0.0
    while args.gate is not None and waited < 120 and busy_fraction() > args.gate:
        time.sleep(2)
        waited += 2
    env = dict(os.environ)
    if mode == 'off':
        env['PHPSTAN_RS_NO_FILE_CACHE'] = '1'
        env['PHPSTAN_RS_NO_STUB_CACHE'] = '1'
    else:
        env['PHPSTAN_RS_CACHE_DIR'] = os.path.join(out, 'cache-' + label)
    if args.machine:
        procs0 = process_ticks()
    path = os.path.join(out, 'out-' + label + '.json')
    with open(path, 'wb') as stdout:
        start = time.perf_counter()
        child = subprocess.Popen([path_bin] + args.argv, cwd=args.cwd, env=env, stdout=stdout,
                                 stderr=subprocess.DEVNULL)
        _, wait_status, usage = os.wait4(child.pid, 0)
        wall = time.perf_counter() - start
    status = child.returncode = os.waitstatus_to_exitcode(wait_status)
    with open(path, 'rb') as fh:
        digest = hashlib.sha256(fh.read()).hexdigest()
    if status != 0 or (args.oracle != 'none' and digest != args.oracle):
        sys.exit(f'{name}: exit {status}, output sha256 {digest}')
    others = None
    if args.machine:
        ours = round((usage.ru_utime + usage.ru_stime) * 100)
        procs1 = process_ticks()
        spent = {}
        for pid, (comm, ticks) in procs1.items():
            before = procs0.get(pid, (comm, 0))[1]
            if pid != child.pid and comm != 'phpstan-rs' and ticks > before:
                spent[comm] = spent.get(comm, 0) + ticks - before
        others = {'total_ticks': sum(spent.values()), 'top': sorted(spent.items(), key=lambda kv: -kv[1])[:5]}
    return {'wall': wall, 'maxrss': usage.ru_maxrss, 'cpu': usage.ru_utime + usage.ru_stime,
            'digest': digest, 'gate_wait': waited, 'others': others}


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


def bootstrap_median_ci(values, reps=10000, seed=7):
    rng = random.Random(seed)
    n = len(values)
    samples = sorted(st.median([values[rng.randrange(n)] for _ in range(n)]) for _ in range(reps))
    return samples[int(0.025 * reps)], samples[int(0.975 * reps)]


def manifest(args):
    def cmd(*argv):
        return subprocess.run(list(argv), capture_output=True, text=True).stdout.strip()
    binaries = {}
    for name in (args.a, args.b):
        path = binary_path(name)
        with open(path, 'rb') as fh:
            binaries[name] = {'path': path, 'sha256': hashlib.sha256(fh.read()).hexdigest()}
    return {
        'started': time.strftime('%Y-%m-%dT%H:%M:%S%z'),
        'argv': sys.argv,
        'git_head': cmd('git', '-C', REPO, 'rev-parse', 'HEAD') or None,
        'git_dirty': bool(cmd('git', '-C', REPO, 'status', '--porcelain', '--untracked-files=no')),
        'mode': args.mode,
        'cwd': args.cwd,
        'command_args': args.argv,
        'oracle': args.oracle,
        'gate': args.gate,
        'machine': args.machine,
        'kernel': cmd('uname', '-r'),
        'nproc': os.cpu_count(),
        'loadavg': load(),
        'binaries': binaries,
    }


def parse_args(argv):
    p = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('out')
    p.add_argument('pairs', type=int)
    p.add_argument('mode', choices=['off', 'warm'])
    p.add_argument('a')
    p.add_argument('b')
    p.add_argument('--cwd', default=PIW)
    p.add_argument('--argv', default=' '.join(CHECK), help='arguments to phpstan-rs, one string')
    p.add_argument('--oracle', default=ORACLE, help="expected output sha256, or 'none' to compare A's and B's outputs")
    p.add_argument('--gate', type=float, help='wait for a busy fraction at most this before each run')
    p.add_argument('--machine', action='store_true', help="record the other processes' CPU per run")
    args = p.parse_args(argv)
    args.argv = shlex.split(args.argv)
    return args


def main():
    args = parse_args(sys.argv[1:])
    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, 'manifest.json'), 'w') as fh:
        json.dump(manifest(args), fh, indent=2)
    a, b, pairs, mode = args.a, args.b, args.pairs, args.mode
    log = open(os.path.join(args.out, 'run.log'), 'a')
    log.write(f'{time.strftime("%Y-%m-%dT%H:%M:%S")} load {load()} start {mode} {a} {b} {pairs} pairs\n')
    for name in (a, b, a, b):
        run(args.out, mode, name, args)
    times, rss, cpu = {a: [], b: []}, {a: [], b: []}, {a: [], b: []}
    others = {a: [], b: []}
    digests = {a: [], b: []}
    gate_waits = 0
    for i in range(pairs):
        order = (a, b) if i % 2 == 0 else (b, a)
        for name in order:
            rec = run(args.out, mode, name, args)
            times[name].append(rec['wall'])
            rss[name].append(rec['maxrss'])
            cpu[name].append(rec['cpu'])
            digests[name].append(rec['digest'])
            gate_waits += rec['gate_wait'] > 0
            if rec['others'] is not None:
                others[name].append(rec['others'])
    if args.oracle == 'none' and digests[a] != digests[b]:
        sys.exit(f'{a} and {b} produced different outputs; the times are not comparable')
    log.write(f'{time.strftime("%Y-%m-%dT%H:%M:%S")} load {load()} done\n')
    log.close()
    with open(os.path.join(args.out, 'times.json'), 'w') as fh:
        json.dump({'mode': mode, 'a': a, 'b': b, 'times': times, 'maxrss_kib': rss, 'cpu': cpu}, fh)
    diffs = [tb - ta for ta, tb in zip(times[a], times[b])]
    for name in (a, b):
        t = times[name]
        print(f'{name}: {len(t)} runs, median {st.median(t):.3f} s, mean {st.mean(t):.3f} s, sd {st.stdev(t):.3f} s; '
              f'peak RSS median {st.median(rss[name]) / 1024:.1f} MiB; CPU median {st.median(cpu[name]):.2f} s')
    w_plus, z, p = wilcoxon(diffs)
    print(f'{b} - {a}, paired: median {st.median(diffs) * 1000:+.1f} ms, mean {st.mean(diffs) * 1000:+.1f} ms, '
          f'{sum(d > 0 for d in diffs)} of {len(diffs)} pairs slower; Wilcoxon W+ {w_plus:.1f}, z {z:.2f}, p {p:.3f}')
    lo, hi = bootstrap_median_ci(diffs)
    print(f'95% bootstrap interval of the median difference: [{lo * 1000:+.1f}, {hi * 1000:+.1f}] ms')
    for label, values, scale, unit in (('peak RSS', rss, 1 / 1024, 'MiB'), ('CPU', cpu, 1, 's')):
        d = [(vb - va) * scale for va, vb in zip(values[a], values[b])]
        w_plus, z, p = wilcoxon(d)
        print(f'{label}, paired: median {st.median(d):+.2f} {unit}, mean {st.mean(d):+.2f} {unit}; '
              f'Wilcoxon W+ {w_plus:.1f}, z {z:.2f}, p {p:.3f}')
    if gate_waits:
        print(f'runs that waited for the load gate: {gate_waits} of {2 * pairs}')
    if args.machine:
        for name in (a, b):
            if not others[name]:
                continue
            totals = [o['total_ticks'] / 100 for o in others[name]]
            names = {}
            for o in others[name]:
                for comm, ticks in o['top']:
                    names[comm] = names.get(comm, 0) + ticks
            print(f'{name}: other processes\' CPU per run median {st.median(totals):.2f} s; top: ' +
                  ', '.join(f'{c} {t / 100:.1f}' for c, t in sorted(names.items(), key=lambda kv: -kv[1])[:5]))


if __name__ == '__main__':
    main()
