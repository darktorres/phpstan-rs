#!/usr/bin/env python3
"""Check the checkpoint's claims against the checked-in evidence and write the result.

    validate_checkpoint.py [--output results/evaluation/checkpoint-validation.json]

Each check reads checked-in records only and records what it found. The file's status is
"passed" only if every check passed. The semantic acceptance is a review judgement, not a
computed value, so it is written as OPEN here.
"""

import argparse
import ast
import gzip
import json
import re
from pathlib import Path


HERE = Path(__file__).resolve().parent
EVIDENCE = HERE / 'results' / 'evaluation'
EXPECTED_SITES = 4642
EXPECTED_OBSERVED = 529
EXPECTED_TARGETS = 307
EXPECTED_LINK_OBJECTS = 525
EXPECTED_C_OBJECTS = 523


def check(name, ok, detail):
    return {'check': name, 'status': 'passed' if ok else 'failed', 'detail': detail}


def python_syntax():
    failed = []
    for path in sorted(HERE.glob('*.py')):
        try:
            ast.parse(path.read_text(), filename=str(path))
        except SyntaxError:
            failed.append(path.name)
    return check('Python syntax', not failed, f'{len(list(HERE.glob("*.py")))} files; failing: {failed}')


def json_and_compressed():
    bad = []
    count = 0
    for path in sorted((HERE / 'results').rglob('*.json')):
        count += 1
        try:
            json.loads(path.read_text())
        except ValueError:
            bad.append(str(path.relative_to(HERE)))
    for path in sorted((HERE / 'results').rglob('*.json.gz')):
        count += 1
        try:
            json.loads(gzip.decompress(path.read_bytes()))
        except ValueError:
            bad.append(str(path.relative_to(HERE)))
    return check('JSON/compressed measurements', not bad, f'{count} records; unreadable: {bad}')


def link_objects():
    manifest = json.loads((EVIDENCE / 'link-manifest.json').read_text())
    objects = manifest['objects']
    c_objects = [o for o in objects if o.get('imported') or o.get('reason') != 'native-assembly-boundary']
    ok = len(objects) == EXPECTED_LINK_OBJECTS and len(c_objects) == EXPECTED_C_OBJECTS
    return check('link objects', ok, f'{len(objects)} link objects, {len(c_objects)} C objects')


def kernel_oom_records():
    records = [
        ('whole-svf-dda (linked module)', 'linked-measurements.json', 'whole-svf-dda'),
        ('whole-compact-dda (campaign)', 'campaign-measurements.json', 'whole-compact-dda'),
    ]
    found = []
    ok = True
    for label, file_name, step in records:
        measured = json.loads((EVIDENCE / file_name).read_text())
        proof = next((r.get('resource_proof') for r in measured if r.get('name') == step), None)
        if proof is None:
            ok = False
            found.append(f'{label}: no proof')
            continue
        events = str(proof.get('memory_events', ''))
        kills = [int(m.group(1)) for m in re.finditer(r'^oom_kill (\d+)$', events, re.M)]
        good = proof.get('kernel_oom_kill') is True and bool(kills) and max(kills) >= 1
        ok &= good
        found.append(f'{label}: kernel_oom_kill={proof.get("kernel_oom_kill")}, oom_kill={kills}')
    return check('both kernel OOM records', ok, '; '.join(found))


def instrumented_sites():
    sites = json.loads((EVIDENCE / 'instrumented-sites.json').read_text())
    count = len(sites['sites'])
    # The instrumented run writes one PTA line per observed call target: site id, then target.
    pta = [line.split() for line in gzip.decompress(
        (EVIDENCE / 'instrumented-hybrid-call-13.pta.gz').read_bytes()).decode().splitlines()
        if line.startswith('PTA ')]
    observed = len({line[1] for line in pta})
    targets = len({line[2] for line in pta})
    ok = count == EXPECTED_SITES and observed == EXPECTED_OBSERVED and targets == EXPECTED_TARGETS
    return check('instrumented sites', ok,
                 f'{count} sites (expected {EXPECTED_SITES}); the hybrid-call run observed {observed} sites '
                 f'(expected {EXPECTED_OBSERVED}) and {targets} targets (expected {EXPECTED_TARGETS})')


def runtime_sentinels():
    # The instrumented run executes runtime.php; its recorded output must name exactly that
    # file's sentinels. The 13-sentinel hybrid-call fixture is a separate, pinned witness.
    recorded = [x.strip() for x in (EVIDENCE / 'instrumented-runtime.out').read_text().splitlines() if x.strip()]
    source = (HERE / 'runtime.php').read_text()
    ok = all(name in source for name in recorded) and len(recorded) == len(set(recorded))
    hybrid = (HERE / 'runtime-hybrid-call-13.php').read_text()
    shared = sum(1 for name in recorded if name in hybrid)
    return check('instrumented runtime sentinels', ok,
                 f'{len(recorded)} sentinels recorded for runtime.php, all named in it; '
                 f'the hybrid-call fixture (13 sentinels) shares {shared} of them')


def cbmc_outcomes():
    lock = json.loads((HERE / 'results/evaluation/lock.json').read_text())
    table = lock.get('expected_outcomes', {})
    measurements = json.loads((EVIDENCE / 'campaign-measurements.json').read_text())
    cbmc = {m['name']: m['exit_code'] for m in measurements if m['name'].startswith('cbmc-')}
    ok = cbmc == {'cbmc-contracts': 0, 'cbmc-invalid-composition': 10, 'cbmc-low-budget': 10}
    return check('CBMC expected pass/fail outcomes', ok, f'recorded exit codes {cbmc}; table lists {sorted(table)}')


def local_links():
    broken = []
    for doc in sorted(HERE.parent.parent.parent.parent.glob('PHP_SEMANTICS_*.md')):
        for target in re.findall(r'\]\((?!https?:)([^)#\s]+)', doc.read_text()):
            if not (doc.parent / target).exists():
                broken.append(f'{doc.name} -> {target}')
    return check('local report links', not broken, f'broken: {broken}')


ROW_TITLE = re.compile(r'^- \*\*(R\d+\.\d+) — (.*?)\*\*')
ACCEPTANCE_VALUES = ('accepted', 'not accepted')


def r_rows(ledger):
    """Read each R row's title and the lines that state its acceptance. The acceptance line
    must directly follow the title; Scope and Remainder may follow it. A row whose acceptance
    line is missing or has another value has no acceptance and is not accepted."""
    lines = ledger.splitlines()
    rows = []
    for index, line in enumerate(lines):
        match = ROW_TITLE.match(line)
        if not match:
            continue
        row = {'id': match.group(1), 'status': match.group(2), 'acceptance': None,
               'scope': None, 'remainder': None}
        if index + 1 < len(lines) and lines[index + 1].startswith('  Acceptance: '):
            value = lines[index + 1][len('  Acceptance: '):].strip()
            if value in ACCEPTANCE_VALUES:
                row['acceptance'] = value
        for text in lines[index + 2:index + 4]:
            for key, prefix in (('scope', '  Scope: '), ('remainder', '  Remainder: ')):
                if text.startswith(prefix):
                    row[key] = text[len(prefix):].strip()
        rows.append(row)
    return rows


def semantic_acceptance(ledger):
    headings = re.findall(r'^### (F\d+\.\d+) — (Open|Partial|Deferred)', ledger, re.M)
    rows = r_rows(ledger)
    open_rows = [{'id': row['id'], 'status': row['status'], 'acceptance': row['acceptance'] or 'unclassified'}
                 for row in rows if row['acceptance'] != 'accepted']
    scoped = [{'id': row['id'], 'scope': row['scope'], 'remainder': row['remainder']}
              for row in rows if row['acceptance'] == 'accepted' and row['scope']]
    if not headings and not open_rows:
        return {'status': 'accepted', 'scoped': scoped}
    return {
        'status': 'OPEN',
        'headings': [{'id': name, 'state': state} for name, state in headings],
        'rows': open_rows,
        'scoped': scoped,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=EVIDENCE / 'checkpoint-validation.json')
    args = parser.parse_args(argv)
    checks = [python_syntax(), json_and_compressed(), link_objects(), kernel_oom_records(),
              instrumented_sites(), runtime_sentinels(), cbmc_outcomes(), local_links()]
    status = 'passed' if all(c['status'] == 'passed' for c in checks) else 'failed'
    ledger = (HERE.parent.parent.parent.parent / 'PHP_SEMANTICS_CATALOG_LEDGER.md').read_text()
    semantic = semantic_acceptance(ledger)
    document = {'status': status, 'checks': checks, 'semantic_acceptance': semantic}
    args.output.write_text(json.dumps(document, indent=2) + '\n')
    for c in checks:
        print(c['status'], c['check'], '-', c['detail'])
    print('status', status)
    return 0 if status == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
