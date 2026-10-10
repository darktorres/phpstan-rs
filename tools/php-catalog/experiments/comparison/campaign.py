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
        'property_records_sha256': 'f23be01e9010e452819fce1e8f424b00a8f27c3e39fdc33303895be5a6ade5b8',
        'failures': set(),
    },
    'cbmc-invalid-composition': {
        'command_exit_code': 10,
        'cprover_status': 'failure',
        'verification_message': 'VERIFICATION FAILED',
        'property_count': 94,
        'inventory_sha256': '9e21a1001834e1caf17db0c6451c1b44e09096fe017586f3d4f10119df713b4b',
        'property_records_sha256': 'efaccde9e2b4d4187b8e4e7ce1014031240840d25d350f8387dc57c6beab143a',
        'failures': {'main.assertion.11'},
    },
    'cbmc-low-budget': {
        'command_exit_code': 10,
        'cprover_status': 'failure',
        'verification_message': 'VERIFICATION FAILED',
        'property_count': 2,
        'inventory_sha256': 'b678de42e03800fdc16c45f27a57319266d309212da908464344a0e1dc7efa74',
        'property_records_sha256': '0f98872fb894bbefb8a6ed24bd3631534d3e3807fcd0e47bbc13eeb266ee13f4',
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


def _property_records_digest(document):
    repository = HERE.parents[3]
    def normalize(value, key=None):
        if isinstance(value, dict):
            return {child_key: normalize(child, child_key) for child_key, child in value.items()}
        if isinstance(value, list):
            return [normalize(child) for child in value]
        if key in ('file', 'workingDirectory') and isinstance(value, str):
            path = Path(value)
            if path.is_absolute():
                try:
                    relative = path.relative_to(repository)
                    return '.' if key == 'workingDirectory' and not relative.parts else str(relative)
                except ValueError:
                    pass
        return value

    records = [
        normalize(prop)
        for item in document
        for prop in item.get('result', [])
    ]
    canonical = json.dumps(
        sorted(records, key=lambda item: item['property']),
        sort_keys=True,
        separators=(',', ':'),
    ).encode()
    return hashlib.sha256(canonical).hexdigest()


def validate_cbmc_inputs(cache, runs, lock_path=None):
    """Check that CBMC and every source file match the follow-up lock record."""
    lock_path = lock_path or HERE / 'results/evaluation/lock.json'
    lock = json.loads(Path(lock_path).read_text())
    followup = lock['followup_cbmc_validation']
    cbmc = Path(cache) / 'deps/usr/bin/cbmc'
    if hashlib.sha256(cbmc.read_bytes()).hexdigest() != followup['cbmc_binary_sha256']:
        raise ValueError('CBMC binary digest differs from the reviewed follow-up lock')
    for name, source, _ in runs:
        locked_source = followup['runs'][name]
        if locked_source['source'] != source:
            raise ValueError(f'{name}: locked source name differs from campaign configuration')
        source_path = HERE / source
        if hashlib.sha256(source_path.read_bytes()).hexdigest() != locked_source['source_sha256']:
            raise ValueError(f'{name}: source digest differs from the reviewed follow-up lock')
        reference = HERE / 'results/evaluation' / (name + '.out')
        if hashlib.sha256(reference.read_bytes()).hexdigest() != locked_source['reviewed_reference_sha256']:
            raise ValueError(f'{name}: checked-in result transcript differs from the reviewed follow-up lock')


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
    if _property_records_digest(actual_document) != expected['property_records_sha256']:
        raise ValueError(f'{name}: property records differ from the reviewed baseline')
    if _property_records_digest(reference_document) != expected['property_records_sha256']:
        raise ValueError(f'{name}: checked-in property records differ from the reviewed baseline')
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
    require_success(runner.run('compact-compile', command, cwd=original, timeout=300), 'compact-compile')
    library = runner.output / 'libSvfLLVM.a'
    shutil.copy2(original / 'lib/libSvfLLVM.a', library)
    require_success(runner.run('compact-archive', ['llvm-ar-21', 'r', library, replacement]), 'compact-archive')
    line = next(x for x in commands if ' -o bin/dvf ' in x)
    command = shlex.split(line.split('&&')[1])
    binary = runner.output / 'dvf-compact'
    command[command.index('-o') + 1] = str(binary)
    command = [str(library) if x == 'lib/libSvfLLVM.a' else x for x in command]
    command = [x for x in command if not x.startswith('--dependency-file=')]
    # The corresponding standalone -Xlinker must be removed as well.
    command = [x for i, x in enumerate(command) if not (x == '-Xlinker' and (i+1 == len(command) or not command[i+1].startswith('-')))]
    require_success(runner.run('compact-link', command, cwd=original, timeout=300), 'compact-link')
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
    validate_cbmc_inputs(cache, cbmc_runs)
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


def require_success(success, step):
    if not success:
        raise RuntimeError(f"campaign step failed: {step}")


def expected_exit_code(step):
    expectation = CBMC_EXPECTATIONS.get(step)
    return 0 if expectation is None else expectation['command_exit_code']


def wait_for_enforcing_worker(report_path, timeout_seconds=6 * 60 * 60,
                              sleep=time.sleep, monotonic=time.monotonic,
                              exists=Path.exists, read_text=Path.read_text):
    """Wait for a published enforcing result; never admit work on missing proof."""
    report_path = Path(report_path)
    deadline = monotonic() + timeout_seconds
    while monotonic() < deadline:
        if exists(report_path):
            try:
                report = json.loads(read_text(report_path))
            except (OSError, json.JSONDecodeError):
                sleep(1)
                continue
            if 'exit_code' in report:
                if report.get('aggregate_hard_limit_enforced') is not True:
                    raise RuntimeError("initial worker published a result without enforced resource proof")
                if not isinstance(report.get('memory_peak_bytes'), int) or not isinstance(
                    report.get('memory_events'), str
                ):
                    raise RuntimeError("initial worker result lacks completed resource measurements")
                return report
            cgroup = report.get('cgroup')
            if cgroup and not Path(cgroup).exists():
                raise RuntimeError("enforcing worker cgroup disappeared before publishing exit_code")
        sleep(1)
    raise TimeoutError(f"timed out waiting for enforcing result: {report_path}")


def complementary(cache, runner, php_binary):
    run_cbmc_campaign(cache, runner)
    require_success(runner.run('smt', [cache/'smt-python/bin/python', HERE/'smt_cases.py', runner.output/'smt-results.json']), 'smt')
    for compiler in ['clang-21', 'gcc']:
        binary = runner.output / ('layout-'+compiler)
        require_success(runner.run('layout-build-'+compiler, [compiler, '-O0', '-g', '-no-pie', HERE/'layout_cases.c', '-o', binary]), 'layout-build-'+compiler)
        require_success(runner.run('layout-runtime-'+compiler, [binary]), 'layout-runtime-'+compiler)
    require_success(runner.run('binary', [cache/'python/bin/python', HERE/'binary_cases.py', '--fixture', runner.output/'layout-clang-21',
        '--php', php_binary,
        '--output', runner.output/'binary-results.json'], timeout=300), 'binary')


def pointer_cases(cache, original, compact, runner):
    for fixture in ['dispatch', 'external', 'layout_cases']:
        bc = runner.output / (fixture+'.bc')
        require_success(runner.run(fixture+'-emit', ['clang-21', '-O0', '-g', '-Xclang', '-disable-O0-optnone',
            '-emit-llvm', '-c', HERE/(fixture+'.c'), '-o', bc]), fixture+'-emit')
        promoted = runner.output / (fixture+'-promoted.bc')
        require_success(runner.run(fixture+'-promote', ['opt-21', '-passes=mem2reg', bc, '-o', promoted]), fixture+'-promote')
        for name, binary in [('stock', original/'bin/dvf'), ('compact', compact)]:
            step = fixture+'-'+name
            require_success(runner.run(step, [binary, '-dfs', '-query=funptr', '-stat', '-print-query-pts', promoted]), step)
    for size in [64, 65536, 16777216, 67108864, 268435456]:
        bc = runner.output / ('growth-'+str(size)+'.bc')
        require_success(runner.run('growth-'+str(size)+'-emit', ['clang-21', '-O0', '-g', '-Xclang', '-disable-O0-optnone',
            '-DPAYLOAD_BYTES='+str(size), '-emit-llvm', '-c', HERE/'dispatch.c', '-o', bc]), 'growth-'+str(size)+'-emit')
        promoted = runner.output / ('growth-'+str(size)+'-promoted.bc')
        require_success(runner.run('growth-'+str(size)+'-promote', ['opt-21', '-passes=mem2reg', bc, '-o', promoted]), 'growth-'+str(size)+'-promote')
        step = 'growth-'+str(size)+'-compact'
        require_success(runner.run(step, [compact, '-dfs', '-query=funptr', '-stat', '-print-query-pts', promoted]), step)
    require_success(runner.run('whole-compact-dda', [compact, '-dfs', '-query=funptr', '-stat', '-print-query-pts',
        cache/'whole-clang/php-cli-promoted.bc'], timeout=1800), 'whole-compact-dda')


def publish_completion(runner):
    measurements_path = runner.output / 'measurements.json'
    measurements = json.loads(measurements_path.read_text())
    names = [record.get('name') for record in measurements]
    if any(not isinstance(name, str) for name in names) or len(set(names)) != len(names):
        raise RuntimeError('cannot publish completion; measurement step names are missing or duplicated')
    failures = [record['name'] for record in measurements
                if record.get('exit_code') != expected_exit_code(record['name'])]
    if failures:
        raise RuntimeError(f"cannot publish completion; failed steps: {failures}")
    manifest = {
        'schema': 1,
        'status': 'complete',
        'step_count': len(measurements),
        'steps': names,
        'expected_exit_codes': {record['name']: expected_exit_code(record['name'])
                                for record in measurements},
        'measurements_sha256': digest(measurements_path),
    }
    (runner.output / 'completed.json').write_text(json.dumps(manifest, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--stock-build', type=Path, required=True)
    parser.add_argument('--php-binary', type=Path, required=True)
    args = parser.parse_args()
    cache, original = args.cache.resolve(), args.stock_build.resolve()
    php_binary = args.php_binary.resolve()
    if not php_binary.is_file():
        parser.error('--php-binary must name the exact measured PHP executable')
    # Admission is serial across campaigns. The first full-link job was started
    # before this script; let its enforcing cgroup finish before admitting more.
    report = cache/'stock-whole-kernel/whole-svf-dda.cgroup.json'
    wait_for_enforcing_worker(report)
    runner = EnforcedRunner(cache/'campaign')
    complementary(cache, runner, php_binary)
    compact = compact_build(cache, original, runner)
    pointer_cases(cache, original, compact, runner)
    publish_completion(runner)


if __name__ == '__main__':
    main()
