#!/usr/bin/env python3
"""SMT-LIB correctness/cutoff/process probes; only valid unsat may prune."""
import json
import subprocess
import sys
from pathlib import Path

worker = Path(__file__).with_name('smt_worker.py')


def may_prune(run):
    answers = [line.strip() for line in run.stdout.splitlines() if line.strip() in ['sat', 'unsat', 'unknown']]
    valid = run.returncode == 0 and '(error' not in run.stdout and not run.stderr and len(answers) == 1
    return valid and answers == ['unsat']


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
    prune = may_prune(run)
    if expected is not None:
        assert valid and answers == [expected], (name, run.stdout, run.stderr)
    else:
        assert not prune, name
    results.append(dict(name=name, smtlib=formula, expected=expected, exit_code=run.returncode,
                        stdout=run.stdout, stderr=run.stderr, may_prune=prune))
# A real child-process crash has no solver result and must retain the path.
failure_program = (
    "import sys; sys.stdin.read(); "
    "print('injected solver crash', file=sys.stderr); raise SystemExit(17)"
)
failure_command = [sys.executable, '-c', failure_program]
failure = subprocess.run(failure_command, input='(check-sat)\n', text=True, capture_output=True)
failure_prune = may_prune(failure)
assert failure.returncode == 17 and failure.stderr.strip() == 'injected solver crash'
assert not failure_prune
results.append(dict(name='process-failure', command=['python', '-c', failure_program],
                    exit_code=failure.returncode, stdout=failure.stdout, stderr=failure.stderr,
                    may_prune=failure_prune, path_retained=not failure_prune))
Path(sys.argv[1]).write_text(json.dumps(results, indent=2) + '\n')
print(json.dumps(dict(cases=len(results), valid_unsat_pruned=sum(x['may_prune'] for x in results))))
