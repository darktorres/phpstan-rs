#!/usr/bin/env python3
"""Serial, enforcing evaluation of reuse adaptations and complementary tools."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import time

from evaluation import EnforcedRunner
from run import HERE, digest


CBMC_EXPECTATIONS = {
    'cbmc-contracts': {
        'command_exit_code': 0,
        'cprover_status': 'success',
        'verification_message': 'VERIFICATION SUCCESSFUL',
        'property_count': 93,
        'inventory_sha256': '6bd71b545e0bc3afbb0656a3d71a5e3b26283140667c9c1ed2ac467788becdaa',
        'failures': set(),
    },
    'cbmc-invalid-composition': {
        'command_exit_code': 10,
        'cprover_status': 'failure',
        'verification_message': 'VERIFICATION FAILED',
        'property_count': 94,
        'inventory_sha256': '9e21a1001834e1caf17db0c6451c1b44e09096fe017586f3d4f10119df713b4b',
        'failures': {'main.assertion.11'},
    },
    'cbmc-low-budget': {
        'command_exit_code': 10,
        'cprover_status': 'failure',
        'verification_message': 'VERIFICATION FAILED',
        'property_count': 2,
        'inventory_sha256': 'b678de42e03800fdc16c45f27a57319266d309212da908464344a0e1dc7efa74',
        'failures': {'main.unwind.0'},
    },
}


def _cbmc_summary(document):
    if not isinstance(document, list):
        raise ValueError('CBMC JSON output must be an array of objects')
    properties = {}
    prover_statuses = []
    verification_messages = []
    for item in document:
        if not isinstance(item, dict):
            raise ValueError('CBMC JSON output contains a non-object item')
        if 'result' in item:
            if not isinstance(item['result'], list):
                raise ValueError('CBMC result field must be an array')
            for prop in item['result']:
                if not isinstance(prop, dict) or not isinstance(prop.get('property'), str):
                    raise ValueError('CBMC property has no stable property identifier')
                identifier = prop['property']
                if identifier in properties:
                    raise ValueError('CBMC emitted duplicate property ' + identifier)
                status = prop.get('status')
                if status not in ('SUCCESS', 'FAILURE', 'UNKNOWN', 'ERROR'):
                    raise ValueError('CBMC property has unexpected status for ' + identifier)
                properties[identifier] = status
        if 'cProverStatus' in item:
            prover_statuses.append(item['cProverStatus'])
        if item.get('messageType') == 'STATUS-MESSAGE' and str(item.get('messageText', '')).startswith('VERIFICATION '):
            verification_messages.append(item['messageText'])
    if len(prover_statuses) != 1:
        raise ValueError('CBMC output must contain exactly one cProverStatus')
    if len(verification_messages) != 1:
        raise ValueError('CBMC output must contain exactly one verification summary')
    return properties, prover_statuses[0], verification_messages[0]


def validate_cbmc_run(name, actual_document, reference_document, command_exit_code):
    """Fail closed unless a bounded CBMC run matches its reviewed result shape."""
    if name not in CBMC_EXPECTATIONS:
        raise ValueError('No reviewed CBMC expectation for ' + name)
    expected = CBMC_EXPECTATIONS[name]
    if command_exit_code != expected['command_exit_code']:
        raise ValueError(
            f'{name}: command exit was {command_exit_code}, '
            f'expected {expected["command_exit_code"]}'
        )
    actual, cprover_status, verification_message = _cbmc_summary(actual_document)
    reference, reference_status, reference_message = _cbmc_summary(reference_document)
    if len(actual) != expected['property_count']:
        raise ValueError(f'{name}: got {len(actual)} properties, expected {expected["property_count"]}')
    actual_inventory = hashlib.sha256(
        json.dumps(sorted(actual.items()), separators=(',', ':')).encode()
    ).hexdigest()
    reference_inventory = hashlib.sha256(
        json.dumps(sorted(reference.items()), separators=(',', ':')).encode()
    ).hexdigest()
    if actual_inventory != expected['inventory_sha256'] or reference_inventory != expected['inventory_sha256']:
        raise ValueError(f'{name}: property inventory digest differs from the reviewed baseline')
    if actual != reference:
        missing = sorted(reference.keys() - actual.keys())
        added = sorted(actual.keys() - reference.keys())
        changed = sorted(key for key in actual.keys() & reference.keys() if actual[key] != reference[key])
        raise ValueError(f'{name}: CBMC property inventory/status changed; missing={missing}, added={added}, changed={changed}')
    failures = {identifier for identifier, status in actual.items() if status != 'SUCCESS'}
    if failures != expected['failures']:
        raise ValueError(f'{name}: got failing/unknown properties {sorted(failures)}, expected {sorted(expected["failures"])}')
    if cprover_status != expected['cprover_status'] or verification_message != expected['verification_message']:
        raise ValueError(f'{name}: unexpected CBMC summary {cprover_status!r} / {verification_message!r}')
    if reference_status != expected['cprover_status'] or reference_message != expected['verification_message']:
        raise ValueError(f'{name}: checked-in reference no longer matches its declared expectation')


def compact_build(cache, original, runner):
    commands = subprocess.check_output(['ninja', '-t', 'commands', 'dvf'], cwd=original, text=True).splitlines()
    compile_line = next(x for x in commands if ' -c ' in x and x.endswith('/lib/LLVMModule.cpp'))
    command = shlex.split(compile_line)
    replacement = runner.output / 'LLVMModule.cpp.o'
    command[command.index('-c') + 1] = str(cache / 'svf-compact/svf-llvm/lib/LLVMModule.cpp')
    command[command.index('-o') + 1] = str(replacement)
    for flag in ['-MT', '-MF']:
        at = command.index(flag); del command[at:at+2]
    assert runner.run('compact-compile', command, cwd=original, timeout=300)
    library = runner.output / 'libSvfLLVM.a'
    shutil.copy2(original / 'lib/libSvfLLVM.a', library)
    assert runner.run('compact-archive', ['llvm-ar-21', 'r', library, replacement])
    line = next(x for x in commands if ' -o bin/dvf ' in x)
    command = shlex.split(line.split('&&')[1])
    binary = runner.output / 'dvf-compact'
    command[command.index('-o') + 1] = str(binary)
    command = [str(library) if x == 'lib/libSvfLLVM.a' else x for x in command]
    command = [x for x in command if not x.startswith('--dependency-file=')]
    # The corresponding standalone -Xlinker must be removed as well.
    command = [x for i, x in enumerate(command) if not (x == '-Xlinker' and (i+1 == len(command) or not command[i+1].startswith('-')))]
    assert runner.run('compact-link', command, cwd=original, timeout=300)
    (runner.output / 'compact-lock.json').write_text(json.dumps(dict(
        source_commit=subprocess.check_output(['git', '-C', str(cache/'svf-compact'), 'rev-parse', 'HEAD'], text=True).strip(),
        patch_sha256=digest(HERE/'svf-compact.patch'), adapted_source_sha256=digest(cache/'svf-compact/svf-llvm/lib/LLVMModule.cpp'),
        reused_library_sha256=digest(original/'lib/libSvfLLVM.a'), binary_sha256=digest(binary),
        unchanged_object_reuse=True, affected_source_files=1), indent=2)+'\n')
    return binary


def run_cbmc_campaign(cache, runner):
    environment = dict(os.environ, LD_LIBRARY_PATH=str(cache/'deps/usr/lib'))
    cbmc = cache / 'deps/usr/bin/cbmc'
    cbmc_runs = [
        ('cbmc-contracts', 'contracts.c', ['--unwind', '4']),
        ('cbmc-invalid-composition', 'contracts.c', ['--unwind', '4', '-DINVALID_COMPOSITION']),
        ('cbmc-low-budget', 'loop_contract.c', ['--unwind', '2']),
    ]
    # Read the committed property inventory before running: the output directory
    # may be reused, and must never become its own oracle.
    references = {
        name: json.loads((HERE / 'results/evaluation' / (name + '.out')).read_text())
        for name, _, _ in cbmc_runs
    }
    for name, source, flags in cbmc_runs:
        runner.run(name, [cbmc, HERE/source, '--unwinding-assertions', '--bounds-check', '--pointer-check',
            '--json-ui', *flags], timeout=300, env=environment)
        actual = json.loads((runner.output / (name + '.out')).read_text())
        exit_code = runner.measurements[-1]['exit_code']
        validate_cbmc_run(name, actual, references[name], exit_code)


def complementary(cache, runner):
    run_cbmc_campaign(cache, runner)
    runner.run('smt', [cache/'smt-python/bin/python', HERE/'smt_cases.py', runner.output/'smt-results.json'])
    for compiler in ['clang-21', 'gcc']:
        binary = runner.output / ('layout-'+compiler)
        assert runner.run('layout-build-'+compiler, [compiler, '-O0', '-g', '-no-pie', HERE/'layout_cases.c', '-o', binary])
        assert runner.run('layout-runtime-'+compiler, [binary])
    runner.run('binary', [cache/'python/bin/python', HERE/'binary_cases.py', '--fixture', runner.output/'layout-clang-21',
        '--php', cache/'php-src-gcc/sapi/cli/php' if (cache/'php-src-gcc/sapi/cli/php').exists() else '/tmp/php-catalog-comparison/php-src/sapi/cli/php',
        '--output', runner.output/'binary-results.json'], timeout=300)


def pointer_cases(cache, original, compact, runner):
    for fixture in ['dispatch', 'external', 'layout_cases']:
        bc = runner.output / (fixture+'.bc')
        assert runner.run(fixture+'-emit', ['clang-21', '-O0', '-g', '-Xclang', '-disable-O0-optnone',
            '-emit-llvm', '-c', HERE/(fixture+'.c'), '-o', bc])
        promoted = runner.output / (fixture+'-promoted.bc')
        assert runner.run(fixture+'-promote', ['opt-21', '-passes=mem2reg', bc, '-o', promoted])
        for name, binary in [('stock', original/'bin/dvf'), ('compact', compact)]:
            runner.run(fixture+'-'+name, [binary, '-dfs', '-query=funptr', '-stat', '-print-query-pts', promoted])
    for size in [64, 65536, 16777216, 67108864, 268435456]:
        bc = runner.output / ('growth-'+str(size)+'.bc')
        assert runner.run('growth-'+str(size)+'-emit', ['clang-21', '-O0', '-g', '-Xclang', '-disable-O0-optnone',
            '-DPAYLOAD_BYTES='+str(size), '-emit-llvm', '-c', HERE/'dispatch.c', '-o', bc])
        promoted = runner.output / ('growth-'+str(size)+'-promoted.bc')
        assert runner.run('growth-'+str(size)+'-promote', ['opt-21', '-passes=mem2reg', bc, '-o', promoted])
        runner.run('growth-'+str(size)+'-compact', [compact, '-dfs', '-query=funptr', '-stat', '-print-query-pts', promoted])
    runner.run('whole-compact-dda', [compact, '-dfs', '-query=funptr', '-stat', '-print-query-pts',
        cache/'whole-clang/php-cli-promoted.bc'], timeout=1800)


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--stock-build', type=Path, required=True)
    args = parser.parse_args()
    cache, original = args.cache.resolve(), args.stock_build.resolve()
    # Admission is serial across campaigns. The first full-link job was started
    # before this script; let its enforcing cgroup finish before admitting more.
    report = cache/'stock-whole-kernel/whole-svf-dda.cgroup.json'
    while report.exists() and 'exit_code' not in json.loads(report.read_text()):
        if not Path(json.loads(report.read_text())['cgroup']).exists():
            break
        time.sleep(1)
    runner = EnforcedRunner(cache/'campaign')
    complementary(cache, runner)
    compact = compact_build(cache, original, runner)
    pointer_cases(cache, original, compact, runner)
    (runner.output/'completed.json').write_text('{}\n')


if __name__ == '__main__':
    main()
