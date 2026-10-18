"""Reads a `perf script --no-inline -F tid,time,ip,sym` dump of a phpstan-rs
recording (`perf record -F 499 -g`, frame pointers). Times are seconds from
the dump's first sample; one sample is 1/hz s of one thread's CPU. A marker
matches a frame that contains it; a regex is searched for in the frame.

<main tid> is the thread that runs the command, named `phpstan-rs-main`; the
process's own main thread only waits for it.

usage: perfdump.py <command> <dump> [args]
  threads                         per thread: first and last sample, samples, roles
  phases <main tid> [window] [hz] per window: cores busy, the role mix, the main
                                  thread's roles; then the main thread's frames
                                  under Session::load, and the role `other`
  unused <t,t,...> [hz] [cores]   per stretch from each t to the next (the last
                                  to the end): (cores x span - CPU s) / cores
  main <main tid> <from> <to>     the main thread's samples in [from, to): time,
                                  the marker frames it is under, the leaf
  incl <marker>...                samples with a frame containing each marker
  leaf <regex>...                 samples whose leaf matches each regex; top leaves
  leafunder <regex> <m,m,...>     samples whose leaf matches, by the nearest
                                  frame above it that contains a marker
  above <regex> <depth>           samples whose leaf matches, by the frames above
  mark <marker> [n] [per s]       samples under a marker: histogram (10 or 100
                                  buckets per second), threads, top n leaves
  split <outer> <inner>...        samples under outer, by the first inner marker
                                  below it (in the order given)
  kernel <from> <to>              samples in [from, to) with a kernel-address leaf
"""
import re
import sys
from collections import Counter, defaultdict

HEAD = re.compile(r'^\s*(\d+)\s+(\d+\.\d+):')
FRAME = re.compile(r'^\s+([0-9a-f]+)\s+(.*?)(?:\+0x[0-9a-f]+)?\s*(?:\(.*\))?$')

# The frame that marks a stretch of the load; the first role whose marker a
# stack holds wins.
ROLES = [
    ('vendor', 'discover_vendor_context'),
    ('stub', 'build_generated_stub_data'),
    ('stub', 'Stubs>::base'),
    ('parse', 'read_indexed'),
    ('finish', 'run_finishing_passes'),
    ('check', 'check_files'),
    ('merge', 'merge_by_shard'),
    ('walk', 'Walk>::run'),
    ('numbered', 'insert_numbered_file'),
    ('bind', 'bind_file'),
    ('load', 'Session>::load'),
]
IDLE = ('rayon_core::sleep', 'rayon_core::registry::WorkerThread::wait_until',
        'sched_yield', 'futex', 'do_sched_yield', 'schedule')
MAIN_MARKS = ('bind_file', 'Stubs>::base', 'split_decl_tables', 'join_decl_tables',
              'merge_by_shard', 'join_project_tables', 'insert_numbered_file',
              'run_finishing_passes', 'check_files', 'expect_files', 'into_parsed',
              'Walk>::finish', 'walk_listing', 'number_files', 'set_path_order',
              'judge_property_types', 'finalize_results', 'vendor', 'PendingVendor',
              'resolved_stubs', 'generated_stubs', 'merge_stubs', 'freeze', 'Walk>::add',
              'add_file')


def samples(path):
    """(tid, time, frames leaf first, the leaf's address) per sample."""
    tid = t = t0 = None
    frames, ips = [], []
    with open(path, errors='replace') as fh:
        for line in fh:
            m = HEAD.match(line)
            if m:
                if tid is not None:
                    yield tid, t - t0, frames, ips[0] if ips else None
                tid, t = int(m.group(1)), float(m.group(2))
                if t0 is None:
                    t0 = t
                frames, ips = [], []
                continue
            if tid is not None and line.strip():
                f = FRAME.match(line)
                if f:
                    ips.append(f.group(1))
                    frames.append(f.group(2).strip())
    if tid is not None:
        yield tid, t - t0, frames, ips[0] if ips else None


def role_of(frames):
    for role, marker in ROLES:
        if any(marker in f for f in frames):
            return role
    if frames and any(any(m in f for m in IDLE) for f in frames[:12]):
        return 'idle'
    return 'other'


def short(name, width):
    name = re.sub(r'<[^<>]*>', '', name)
    name = re.sub(r'<[^<>]*>', '', name)
    return name[:width]


def threads(path):
    per = {}
    for tid, t, frames, _ in samples(path):
        s = per.setdefault(tid, {'first': t, 'last': t, 'n': 0, 'roles': Counter()})
        s['last'] = t
        s['n'] += 1
        s['roles'][role_of(frames)] += 1
    print(f'{"tid":>8} {"first":>8} {"last":>8} {"samples":>8}  roles')
    for tid, s in sorted(per.items(), key=lambda kv: kv[1]['first']):
        roles = ', '.join(f'{r} {c}' for r, c in s['roles'].most_common(4))
        print(f'{tid:>8} {s["first"]:8.3f} {s["last"]:8.3f} {s["n"]:8,}  {roles}')


def phases(path, main, window=0.1, hz=499.0):
    win = defaultdict(Counter)
    main_win = defaultdict(Counter)
    main_load = Counter()
    other_leaf = Counter()
    other_frame = Counter()
    n = 0
    for tid, t, frames, _ in samples(path):
        n += 1
        w = int(t / window)
        role = role_of(frames)
        win[w][role] += 1
        if tid == main:
            main_win[w][role] += 1
            if any('Session>::load' in f for f in frames):
                deep = next((f for f in frames if 'phpstan_rs' in f), frames[0] if frames else '?')
                main_load[(short(deep, 90), short(frames[0], 90) if frames else '?')] += 1
        elif role == 'other':
            other_leaf[short(frames[0], 90) if frames else '?'] += 1
            deep = next((f for f in frames if 'phpstan_rs' in f), None)
            other_frame[short(deep, 90) if deep else '(no phpstan frame)'] += 1
    per = hz * window
    print(f'{n:,} samples; window {window} s; cores = samples / {per:.1f}')
    print(f'{"t":>5} {"cores":>5}  roles (all threads)  ||  main thread')
    for w in range(max(win) + 1):
        busy = sum(win[w].values()) / per
        mix = ' '.join(f'{r}:{c / per:.1f}' for r, c in win[w].most_common())
        mm = ' '.join(f'{r}:{c}' for r, c in main_win[w].most_common())
        print(f'{w * window:5.2f} {busy:5.2f}  {mix}  ||  {mm}')
    print('\n-- main thread under Session::load: deepest phpstan frame, leaf (samples)')
    for (deep, leaf), c in main_load.most_common(40):
        print(f'{c:5}  {deep}  <-  {leaf}')
    print('\n-- pool/other threads, role other: deepest phpstan frame')
    for k, c in other_frame.most_common(25):
        print(f'{c:5}  {k}')
    print('\n-- pool/other threads, role other: leaf')
    for k, c in other_leaf.most_common(25):
        print(f'{c:5}  {k}')


def unused(path, edges, hz=499.0, cores=8):
    rel = [t for _, t, _, _ in samples(path)]
    end = max(rel)
    edges = edges + [end]
    total = 0.0
    print(f'{len(rel):,} samples, last at {end:.3f} s, busy {len(rel) / hz:.3f} core-s')
    for a, b in zip(edges, edges[1:]):
        busy = sum(1 for t in rel if a <= t < b) / hz
        span = b - a
        left = (cores * span - busy) / cores
        total += left
        print(f'{a:6.3f} to {b:6.3f}  span {span:5.3f}  cores {busy / span:4.2f}  unused {left:5.3f} s')
    print(f'total unused {total:.3f} s; run {end:.3f} - busy/{cores} {len(rel) / hz / cores:.3f}'
          f' = {end - len(rel) / hz / cores:.3f}')


def main_thread(path, main, a, b):
    for tid, t, frames, _ in samples(path):
        if tid == main and a <= t < b:
            hits = [m for m in MAIN_MARKS if any(m in f for f in frames)]
            leaf = frames[0][:50] if frames else '?'
            print(f'{t:7.3f}  {",".join(hits[:3]):60}  {leaf}')


def incl(path, marks):
    inc = Counter()
    n = 0
    for _, _, frames, _ in samples(path):
        n += 1
        for m in marks:
            if any(m in f for f in frames):
                inc[m] += 1
    print(f'{n} samples; inclusive counts (2 ms of CPU each at 499 Hz)')
    for m in marks:
        print(f'{inc[m]:6}  {m}')


def leaf(path, patterns):
    pats = [(p, re.compile(p)) for p in patterns]
    leaves = Counter()
    want = Counter()
    n = 0
    for _, _, frames, _ in samples(path):
        n += 1
        if not frames:
            continue
        leaves[frames[0]] += 1
        for p, rx in pats:
            if rx.search(frames[0]):
                want[p] += 1
    print(f'{n} samples')
    for p, _ in pats:
        print(f'{want[p]:6}  /{p}/')
    print('-- top leaves')
    for k, c in leaves.most_common(25):
        print(f'{c:6}  {k[:110]}')


def leafunder(path, pattern, markers):
    rx = re.compile(pattern)
    by = Counter()
    total = 0
    for _, _, frames, _ in samples(path):
        if frames and rx.search(frames[0]):
            total += 1
            by[next((m for f in frames[1:] for m in markers if m in f), '(none)')] += 1
    print(f'{total} samples with leaf /{pattern}/')
    for k, c in by.most_common():
        print(f'{c:6}  {k}')


def above(path, pattern, depth):
    rx = re.compile(pattern)
    by = Counter()
    for _, _, frames, _ in samples(path):
        if frames and rx.search(frames[0]):
            by[' | '.join(short(f, 60) for f in frames[1:1 + depth])] += 1
    for k, c in by.most_common(20):
        print(f'{c:5}  {k}')


def mark(path, marker, nleaf=12, per=10):
    hist = Counter()
    tids = Counter()
    leaves = Counter()
    for tid, t, frames, _ in samples(path):
        if any(marker in f for f in frames):
            hist[int(t * per)] += 1
            tids[tid] += 1
            leaves[short(frames[0], 80) if frames else '?'] += 1
    digits = len(str(per)) - 1
    print(f'{marker}: {sum(hist.values())} samples; threads {len(tids)}')
    print('  time: ' + ' '.join(f'{k / per:.{digits}f}:{v}' for k, v in sorted(hist.items())))
    for k, c in leaves.most_common(nleaf):
        print(f'  {c:5}  {k}')


def split(path, outer, inner):
    by = Counter()
    child = Counter()
    for _, _, frames, _ in samples(path):
        idx = next((i for i, f in enumerate(frames) if outer in f), None)
        if idx is None:
            continue
        hit = next((m for m in inner if any(m in f for f in frames[:idx])), '(other)')
        by[hit] += 1
        if hit == '(other)' and idx > 0:
            child[short(frames[idx - 1], 70)] += 1
    print(f'under {outer}: {sum(by.values())}')
    for k, c in by.most_common():
        print(f'{c:6}  {k}')
    print('  (other) by the frame right below the outer one:')
    for k, c in child.most_common(10):
        print(f'  {c:6}  {k}')


def kernel(path, a, b):
    n = k = ipi = 0
    leaves = Counter()
    for _, t, frames, ip in samples(path):
        if a <= t < b and frames:
            n += 1
            if ip.startswith('ffff'):
                k += 1
                leaves[frames[0][:40]] += 1
                if any('smp_call_function' in f or 'flush_tlb' in f or 'send_IPI' in f
                       for f in frames):
                    ipi += 1
    print(f'{a}-{b} s: {n} samples, kernel leaf {k} ({k / max(n, 1) * 100:.0f}%), '
          f'TLB or IPI stacks {ipi} ({ipi / max(n, 1) * 100:.0f}%)')
    for name, c in leaves.most_common(8):
        print(f'{c:6}  {name}')


def run(argv):
    if len(argv) < 3:
        raise SystemExit(__doc__)
    cmd, path, args = argv[1], argv[2], argv[3:]
    if cmd == 'threads':
        threads(path)
    elif cmd == 'phases':
        phases(path, int(args[0]), *(float(x) for x in args[1:3]))
    elif cmd == 'unused':
        unused(path, [float(x) for x in args[0].split(',')],
               float(args[1]) if len(args) > 1 else 499.0,
               int(args[2]) if len(args) > 2 else 8)
    elif cmd == 'main':
        main_thread(path, int(args[0]), float(args[1]), float(args[2]))
    elif cmd == 'incl':
        incl(path, args)
    elif cmd == 'leaf':
        leaf(path, args)
    elif cmd == 'leafunder':
        leafunder(path, args[0], args[1].split(','))
    elif cmd == 'above':
        above(path, args[0], int(args[1]))
    elif cmd == 'mark':
        mark(path, args[0], *(int(x) for x in args[1:3]))
    elif cmd == 'split':
        split(path, args[0], args[1:])
    elif cmd == 'kernel':
        kernel(path, float(args[0]), float(args[1]))
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    run(sys.argv)
