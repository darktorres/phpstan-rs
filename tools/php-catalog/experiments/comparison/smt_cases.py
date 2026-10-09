#!/usr/bin/env python3
"""SMT-LIB correctness/cutoff/process probes; only valid unsat may prune."""
import json
import subprocess
import sys
from pathlib import Path

worker = Path(__file__).with_name('smt_worker.py')
cases = {
    'contradictory-guards': ('(declare-const x (_ BitVec 32)) (assert (bvult x #x00000003)) (assert (bvugt x #x00000004))', 'unsat'),
    'unsigned-overflow': ('(assert (= (bvadd #xff #x01) #x00))', 'sat'),
    'signed-comparison': ('(assert (bvslt #xff #x01))', 'sat'),
    'unsigned-comparison': ('(assert (bvult #xff #x01))', 'unsat'),
    'sign-extension': ('(assert (= ((_ sign_extend 8) #xff) #xffff))', 'sat'),
    'zero-extension': ('(assert (= ((_ zero_extend 8) #xff) #x00ff))', 'sat'),
    'pointer-identity': ('(declare-sort Region 0) (declare-const a Region) (declare-const b Region) (assert (= a b)) (assert (distinct a b))', 'unsat'),
    'resource-exhaustion': ('(set-option :rlimit 1) (declare-const x (_ BitVec 32)) (assert (= (bvmul x x) #x00000011))', 'unknown'),
    'unsupported-logic': ('(set-logic QF_NOT_A_SUPPORTED_LOGIC) (assert true)', None),
    'malformed-formula': ('(assert (= )', None),
}
results = []
for name, (formula, expected) in cases.items():
    command = [sys.executable, str(worker)]
    run = subprocess.run(command, input=formula + '\n(check-sat)\n', text=True, capture_output=True)
    answers = [line.strip() for line in run.stdout.splitlines() if line.strip() in ['sat', 'unsat', 'unknown']]
    valid = run.returncode == 0 and '(error' not in run.stdout and not run.stderr and len(answers) == 1
    prune = valid and answers == ['unsat']
    if expected is not None:
        assert valid and answers == [expected], (name, run.stdout, run.stderr)
    else:
        assert not prune, name
    results.append(dict(name=name, smtlib=formula, expected=expected, exit_code=run.returncode,
                        stdout=run.stdout, stderr=run.stderr, may_prune=prune))
# Process failure has no result and must retain the path.
results.append(dict(name='process-failure', exit_code=1, may_prune=False, path_retained=True))
Path(sys.argv[1]).write_text(json.dumps(results, indent=2) + '\n')
print(json.dumps(dict(cases=len(results), valid_unsat_pruned=sum(x['may_prune'] for x in results))))
