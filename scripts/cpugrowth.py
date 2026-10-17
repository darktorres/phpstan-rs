"""Where the CPU time of a run grows with the thread count: two dumps of the
same command (`perf script --no-inline -F tid,time,ip,sym`, task-clock at the
same rate), the first at fewer threads. Each sample is put in one class by its
leaf, then the classes and the roles (perfdump.ROLES) are compared.

usage: cpugrowth.py <dump at few threads> <dump at more threads> [hz]
"""
import os
import re
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from perfdump import IDLE, role_of, samples, short  # noqa: E402

KERNEL = [
    ('kernel: page zeroing', ('__clear_pages', 'clear_page')),
    ('kernel: TLB shootdown, IPI', ('smp_call_function', 'flush_tlb', 'send_IPI',
                                    'native_flush_tlb', 'llist_add_batch')),
    ('kernel: task switch', ('finish_task_switch', '__schedule', 'schedule',
                             'preempt_schedule', 'pick_next')),
    ('kernel: futex, yield', ('futex', 'sched_yield', 'do_sched_yield')),
    ('kernel: softirq, irq exit', ('handle_softirqs', 'irqentry_exit', '__do_softirq')),
    ('kernel: compaction, migration', ('compact', 'migrate', 'isolate_')),
    ('kernel: file system', ('d_lookup', 'link_path_walk', 'statx', 'getdents',
                             'filemap', 'copy_page_to_iter', 'rep_movs')),
]
SPIN = ('rayon_core', 'crossbeam', 'std::thread::yield_now', 'sched_yield',
        'core::hint::spin_loop', '__sched_yield')


def classify(frames, ip):
    leaf = frames[0] if frames else '?'
    if ip and ip.startswith('ffff'):
        for name, keys in KERNEL:
            if any(k in leaf for k in keys):
                return name
        return 'kernel: other'
    if any(k in leaf for k in SPIN) or (
            any(any(m in f for m in IDLE) for f in frames[:12])
            and not any('phpstan_rs' in f or 'syntax::' in f or 'types::' in f
                        for f in frames[:6])):
        return 'user: pool idle, spinning'
    if re.search(r'(^|[^a-z])_?mi_', leaf):
        return 'user: mimalloc'
    if 'memmove' in leaf or 'memcpy' in leaf or 'memset' in leaf or 'memcmp' in leaf:
        return 'user: libc mem*'
    return 'user: our code'


def tally(path):
    cls = Counter()
    role = Counter()
    both = Counter()
    leaf_by_cls = {}
    for _, _, frames, ip in samples(path):
        c = classify(frames, ip)
        r = role_of(frames)
        cls[c] += 1
        role[r] += 1
        both[(r, c)] += 1
        leaf_by_cls.setdefault(c, Counter())[short(frames[0], 70) if frames else '?'] += 1
    return cls, role, both, leaf_by_cls


def table(title, a, b, hz):
    keys = sorted(set(a) | set(b), key=lambda k: (-(b.get(k, 0) - a.get(k, 0)), str(k)))
    print(f'\n-- {title} (CPU seconds; {hz:.0f} samples per second)')
    print(f'{"":36} {"few":>7} {"more":>7} {"more-few":>9}')
    for k in keys:
        x, y = a.get(k, 0) / hz, b.get(k, 0) / hz
        name = k if isinstance(k, str) else ' / '.join(k)
        print(f'{name[:36]:36} {x:7.3f} {y:7.3f} {y - x:+9.3f}')
    x, y = sum(a.values()) / hz, sum(b.values()) / hz
    print(f'{"total":36} {x:7.3f} {y:7.3f} {y - x:+9.3f}')


def main():
    few, more = sys.argv[1], sys.argv[2]
    hz = float(sys.argv[3]) if len(sys.argv) > 3 else 499.0
    ca, ra, ba, la = tally(few)
    cb, rb, bb, lb = tally(more)
    table('by class of leaf', ca, cb, hz)
    table('by role', ra, rb, hz)
    table('by role and class (largest growth first)', ba, bb, hz)
    for c in ('user: pool idle, spinning', 'kernel: other', 'user: our code', 'user: mimalloc'):
        print(f'\n-- {c}: top leaves, few / more')
        keys = sorted(set(la.get(c, {})) | set(lb.get(c, {})),
                      key=lambda k: (-(lb.get(c, Counter())[k] - la.get(c, Counter())[k]), k))
        for k in keys[:15]:
            print(f'{la.get(c, Counter())[k]:6} {lb.get(c, Counter())[k]:6}  {k}')


if __name__ == '__main__':
    main()
