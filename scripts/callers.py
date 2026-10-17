"""Inclusive Ir of calls from each caller to the callees whose name matches a
regex, summed over a run directory's `cg.out-*` files (callgrind with
`--separate-threads=yes`, one file per thread).

usage: callers.py <run dir> <regex> [n]
"""
import glob, os, re, sys
from collections import defaultdict

def parse(path, pat, edges):
    names = {}
    def resolve(text):
        if text.startswith('('):
            ident, _, rest = text.partition(')')
            key = ident + ')'
            if rest.strip():
                names[key] = rest.strip()
            return names.get(key, key)
        return text
    fn = cfn = None
    in_call = False
    with open(path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            if line.startswith('fn='):
                fn = resolve(line[3:].strip())
            elif line.startswith('cfn='):
                cfn = resolve(line[4:].strip())
            elif line.startswith('calls='):
                in_call = True
            elif in_call and (line[:1].isdigit() or line[:1] in '+-*'):
                cost = int(line.split()[-1])
                if pat.search(cfn or ''):
                    edges[(fn, cfn)] += cost
                in_call = False

def run(d, pat):
    edges = defaultdict(int)
    for f in sorted(glob.glob(os.path.join(d, 'cg.out-*'))):
        parse(f, pat, edges)
    return edges

pat = re.compile(sys.argv[2])
n = int(sys.argv[3]) if len(sys.argv) > 3 else 15
e = run(sys.argv[1], pat)
by_caller = defaultdict(int)
for (a, b), c in e.items():
    by_caller[a] += c
for a, c in sorted(by_caller.items(), key=lambda kv: -kv[1])[:n]:
    print(f'{c:16,}  {a[:150]}')
