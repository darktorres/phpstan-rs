#!/usr/bin/env python3
"""Baseline call graphs over textual LLVM IR, for builtin -> user-code-mechanism reachability.
B0: an indirect call may reach every address-taken function (sound unless code is called through
    integers, inline assembly or dlopen).
B1: an indirect call may reach the address-taken functions whose LLVM function type equals the
    call's (sound unless a function is called through a pointer of another type).
Usage: baseline_cg.py FILE.ll OUT.json"""
import json, re, sys, collections

ll, out = sys.argv[1], sys.argv[2]
define_re = re.compile(r'^define\s+(?:[^@]*?)\s(\S+)\s+@("?[^"(]+"?)\((.*?)\)\s*(?:#\d+|\w|$)')
global_fn_ref = re.compile(r'@("?[A-Za-z_.$][\w.$]*"?)')
call_re = re.compile(r'\b(?:call|invoke)\b(.*)')

def norm_params(s):
    # "ptr noundef %0, i64 %1" -> ["ptr", "i64"]; keep "..." for varargs
    out, depth, cur = [], 0, ''
    for ch in s:
        if ch in '({[<': depth += 1
        if ch in ')}]>': depth -= 1
        if ch == ',' and depth == 0:
            out.append(cur); cur = ''
        else:
            cur += ch
    if cur.strip(): out.append(cur)
    types = []
    for p in out:
        p = p.strip()
        if p == '...': types.append('...'); continue
        t = p.split()[0] if p else ''
        types.append(t)
    return tuple(types)

functions = {}          # name -> (ret, params)
direct = collections.defaultdict(set)
indirect = collections.defaultdict(list)   # caller -> [(ret, params)]
address_taken = set()
current = None
declared = set()
for line in open(ll, errors='replace'):
    if line.startswith('define '):
        m = define_re.match(line)
        if not m:
            current = None
            continue
        ret, name, params = m.group(1), m.group(2).strip('"'), m.group(3)
        functions[name] = (ret, norm_params(params))
        current = name
        continue
    if line.startswith('declare '):
        m = re.search(r'@("?[^"(]+"?)\(', line)
        if m: declared.add(m.group(1).strip('"'))
        continue
    if line.startswith('}'):
        current = None
        continue
    if current is None:
        # globals: function addresses in initializers are address-taken
        if line.startswith('@') and '=' in line:
            for ref in global_fn_ref.findall(line.split('=', 1)[1]):
                address_taken.add(ref.strip('"'))
        continue
    m = call_re.search(line)
    callee_name = None
    if m:
        rest = m.group(1)
        dm = re.search(r'@("?[\w.$]+"?)\(', rest)
        im = re.search(r'\s%[\w.$]+\(', rest)
        if dm and (not im or rest.index(dm.group(0)) < rest.index(im.group(0))):
            callee_name = dm.group(1).strip('"')
            direct[current].add(callee_name)
        elif im:
            # function type of an indirect call: "<ret> [(params)] %reg(args)"
            head = rest[:rest.index(im.group(0))]
            argstr = rest[rest.index(im.group(0)) + len(im.group(0)):]
            depth = 1; end = 0
            for i, ch in enumerate(argstr):
                if ch == '(': depth += 1
                elif ch == ')':
                    depth -= 1
                    if depth == 0: end = i; break
            sig = re.search(r'(\S+)\s*\(([^()]*)\)\s*$', head.strip())
            if sig:
                ret, params = sig.group(1), norm_params(sig.group(2))
            else:
                toks = head.strip().split()
                ret = toks[-1] if toks else 'void'
                params = norm_params(re.sub(r'%[\w.$]+', '', argstr[:end]))
            indirect[current].append((ret, params))
    # every other function reference in the body is an address taken
    for ref in global_fn_ref.findall(line):
        ref = ref.strip('"')
        if ref != callee_name:
            address_taken.add(ref)

address_taken &= set(functions) | declared
by_type = collections.defaultdict(set)
for f in address_taken:
    if f in functions:
        by_type[functions[f]].add(f)

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

def successors(f, mode):
    s = set(direct.get(f, ()))
    for (ret, params) in indirect.get(f, ()):
        if mode == 'B0':
            s |= address_taken
        else:
            s |= by_type.get((ret, params), set())
    return s

result = {'functions': len(functions), 'address_taken': len(address_taken),
          'indirect_sites': sum(len(v) for v in indirect.values())}
roots = sorted(f for f in functions if f.startswith('zif_') or f.startswith('zim_'))
result['roots'] = len(roots)
for mode in ('B1', 'B0'):
    # reachability per root, stopping at user-code entry points
    cache = {}
    reach = {}
    succ = {f: successors(f, mode) for f in functions}
    # one SCC-free BFS per root is too slow for B0; reach is the same set for most roots there,
    # so compute the closure of every function once with an iterative worklist over reversed edges
    mechs_of = {}
    for f in functions:
        m = mech(f)
        if m: mechs_of[f] = {m}
    # fixed point: mechs(f) = own ∪ ⋃ mechs(g) for g in succ(f), not passing through user entry
    changed = True
    known = {f: set(mechs_of.get(f, ())) for f in functions}
    preds = collections.defaultdict(set)
    for f, ss in succ.items():
        if f in USER_ENTRY: continue
        for g in ss:
            if g in functions: preds[g].add(f)
    work = collections.deque(f for f in functions if known[f])
    while work:
        g = work.popleft()
        for f in preds[g]:
            if not known[g] <= known[f]:
                known[f] |= known[g]
                work.append(f)
    result[mode] = {r: sorted(known[r]) for r in roots}
json.dump(result, open(out, 'w'), indent=0)
print(json.dumps({k: v for k, v in result.items() if k not in ('B0', 'B1')}))
for mode in ('B1', 'B0'):
    counts = collections.Counter(m for r in roots for m in result[mode][r])
    print(mode, dict(counts.most_common()))
