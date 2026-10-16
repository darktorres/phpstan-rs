#!/usr/bin/env python3
"""Scores a builtin -> mechanism reachability result against what PHP was seen doing.
Truth: the tracer's observations (exp3 all-observed.tsv) mapped to mechanisms, for builtins that
exist in the analysed build. Usage: score_reach.py REACH.tsv(builtin\tmechanism) TRACE_OBSERVED.tsv ROOTS.txt"""
import sys, collections

reach_path, observed_path, roots_path = sys.argv[1:4]
roots = {l.strip() for l in open(roots_path) if l.strip()}   # analysed builtins, as C names

def c_name(php_name):
    # strlen -> zif_strlen; class::method -> zim_class_method (case kept loose)
    if '::' in php_name:
        c, m = php_name.split('::', 1)
        return 'zim_' + c.replace('\\', '_') + '_' + m
    return 'zif_' + php_name

MAGIC = {'__get': '__get', '__set': '__set', '__isset': '__isset', '__unset': '__unset',
         '__tostring': '__toString', '__debuginfo': '__debugInfo', '__destruct': '__destruct'}
ITER = {'getiterator', 'valid', 'current', 'key', 'next', 'rewind'}
WRAP = {'rename': 'wrapper:rename', 'unlink': 'wrapper:unlink', 'mkdir': 'wrapper:mkdir', 'rmdir': 'wrapper:rmdir',
        'url_stat': 'wrapper:stat_url', 'stream_open': 'wrapper:opener', 'dir_opendir': 'wrapper:opendir',
        'stream_metadata': 'wrapper:metadata', 'stream_read': 'stream:read', 'stream_write': 'stream:write',
        'stream_close': 'stream:close', 'stream_flush': 'stream:flush', 'stream_seek': 'stream:seek',
        'stream_stat': 'stream:stat', 'stream_set_option': 'stream:set_option', 'dir_readdir': 'stream:readdir',
        'dir_closedir': 'stream:closedir', 'dir_rewinddir': 'stream:rewinddir', 'stream_cast': 'stream:cast'}

truth = collections.defaultdict(set)
lower_roots = {r.lower(): r for r in roots}
for line in list(open(observed_path))[1:]:
    fn, place, kind, detail = line.split('\t')[:4]
    c = c_name(fn).lower()
    if c not in lower_roots:
        continue
    root = lower_roots[c]
    if kind == 'method' and detail in MAGIC:
        truth[root].add(MAGIC[detail])
    elif kind == 'method' and detail in ITER:
        truth[root].add('iterator')
    elif kind == 'wrapper' and detail in WRAP:
        truth[root].add(WRAP[detail])
    truth[root].add('any-user-code')

reach = collections.defaultdict(set)
for line in open(reach_path):
    parts = line.rstrip('\n').split('\t')
    if len(parts) >= 2:
        reach[parts[0]].add(parts[1])

pairs = sum(len(v) for v in truth.values())
missed = [(r, m) for r, ms in sorted(truth.items()) for m in sorted(ms) if m not in reach.get(r, set())]
mechs = sorted({m for v in reach.values() for m in v} | {m for v in truth.values() for m in v})
unreached = sum(1 for r in roots for m in mechs if m not in reach.get(r, set()))
print(f'roots {len(roots)}  observed pairs {pairs}  missed {len(missed)}  recall {1 - len(missed) / max(pairs, 1):.3f}')
print(f'mechanisms {len(mechs)}  root x mechanism pairs claimed unreachable {unreached} of {len(roots) * len(mechs)}')
for r, m in missed[:40]:
    print('  missed', r, m)
