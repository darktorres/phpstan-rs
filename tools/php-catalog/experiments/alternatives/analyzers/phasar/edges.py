#!/usr/bin/env python3
"""Turns Phasar's psr-cg.json (callee -> call-site instruction ids) into builtin -> mechanism
reachability, numbering the module the way LLVMProjectIRDB does: global variables first, then every
instruction of every function in module order. Usage: edges.py MODULE.ll psr-cg.json OUT.tsv"""
import json, re, sys, bisect, collections
ll, cg, out = sys.argv[1:4]
globals_count = 0
starts, names = [], []
next_id = None
in_fn = False
for line in open(ll, errors='replace'):
    if not in_fn:
        if line.startswith('@') and re.match(r'^@[^ ]+ = ', line) and not re.search(r' = [^"]*\b(alias|ifunc)\b', line):
            globals_count += 1
        elif line.startswith('define '):
            if next_id is None:
                next_id = globals_count
            m = re.search(r'@("?[^"(]+"?)\(', line)
            starts.append(next_id); names.append(m.group(1).strip('"'))
            in_fn = True
        continue
    if line.startswith('}'):
        in_fn = False
        continue
    s = line.strip()
    if not s or s.startswith(';') or (not line.startswith(' ') and s.endswith(':')) or re.match(r'^[\w.$"-]+:(\s*;.*)?$', s):
        continue
    if re.match(r'^  \S', line) and not s.startswith('#dbg_') and not s.startswith(']'):
        next_id += 1
print('globals', globals_count, 'functions', len(names), 'last id', next_id)
d = json.load(open(cg))
edges = collections.defaultdict(set)
for callee, sites in d.items():
    if not isinstance(sites, list):
        continue
    for i in sites:
        k = bisect.bisect_right(starts, i) - 1
        if k >= 0:
            edges[names[k]].add(callee)
USER_ENTRY = {'zend_call_function', 'execute_ex', 'zend_execute', 'zend_execute_ex', 'zend_execute_scripts'}
MECH = {'zend_std_call_getter': '__get', 'zend_std_call_setter': '__set', 'zend_std_call_issetter': '__isset',
        'zend_std_call_unsetter': '__unset', 'zend_std_cast_object_tostring': '__toString',
        'zend_std_get_debug_info': '__debugInfo', 'zend_objects_destroy_object': '__destruct',
        'zend_lazy_object_init': 'lazy-initializer', 'zend_user_serialize': 'Serializable',
        'zend_user_unserialize': 'Serializable', 'zend_call_function': 'any-user-code'}
def mech(f):
    if f in MECH: return MECH[f]
    if f.startswith('zend_user_it_'): return 'iterator'
    if f.startswith('user_wrapper_'): return 'wrapper:' + f[13:]
    if f.startswith('php_userstreamop_'): return 'stream:' + f[17:]
    return None
known = collections.defaultdict(set)
preds = collections.defaultdict(set)
for f, cs in edges.items():
    if f in USER_ENTRY: continue
    for g in cs: preds[g].add(f)
allf = set(edges) | {g for cs in edges.values() for g in cs}
for f in allf:
    m = mech(f)
    if m: known[f].add(m)
work = collections.deque(f for f in allf if known[f])
while work:
    g = work.popleft()
    for f in preds[g]:
        if not known[g] <= known[f]:
            known[f] |= known[g]; work.append(f)
roots = sorted(f for f in allf if f.startswith('zif_') or f.startswith('zim_'))
with open(out, 'w') as o:
    for r in roots:
        for m in sorted(known[r]): o.write(f'{r}\t{m}\n')
c = collections.Counter(m for r in roots for m in known[r])
print('roots', len(roots)); print(sorted(c.items(), key=lambda x: -x[1]))
for f in ['zif_rename', 'zif_array_column', 'zif_abs', 'zif_strlen']:
    print(f, sorted(known[f]))
