#!/usr/bin/env python3
"""Check bounded comparison evidence, including known backend limitations.

These checks establish importer/framework facts, never catalog soundness.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path


def read_function(path, name):
    return next(f for f in json.loads(path.read_text())['functions'] if f['name'] == name)


def bypasses_cleanup(function, origin, destination, cleanup):
    blocks = {b['id']: b for b in function['blocks']}
    starts = [b for b in function['blocks'] if any(origin(s) for s in b['statements'])]
    assert len(starts) == 1
    # Both exporters preserve the configured normal/exception successor order
    # for this fixture. Inspect the exceptional edge specifically.
    successors = starts[0]['successors']
    assert len(successors) == 2
    queue = [successors[1]]
    seen = set()
    while queue:
        node = queue.pop()
        if node is None or node in seen:
            continue
        seen.add(node)
        block = blocks[node]
        for statement in block['statements']:
            if cleanup(statement):
                break
            if destination(statement):
                return True
        else:
            queue.extend(block['successors'])
    return False


def stats(text):
    result = {}
    for key in ['TotalObjects', 'TotalPointers', 'TotalSVFStmts', 'NumOfQuery',
                'NumOfOOBQuery', 'PointsToBlkPtr', 'TotalNode', 'TotalEdge']:
        result[key] = re.findall(r'^' + key + r'\s+([^\n]+)', text, re.M)
    return result


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--results', type=Path, required=True)
    parser.add_argument('--aux', type=Path, required=True)
    parser.add_argument('--external', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root, aux = args.results, args.aux
    findings = {}
    ast = read_function(root / 'cleanup-ast-0.out', 'cleanup')
    llvm = read_function(root / 'cleanup-llvm-read-0.out', 'cleanup')
    findings['clang_cfg_unwind_bypasses_destructor'] = bypasses_cleanup(
        ast, lambda s:s.get('callee') == 'may_throw',
        lambda s:s.get('text') == 'event(4)', lambda s:s.get('kind') == 'ImplicitDtor')
    findings['llvm_unwind_bypasses_destructor'] = bypasses_cleanup(
        llvm, lambda s:s.get('callee') == '_Z9may_throwv',
        lambda s:s.get('callee') == '__cxa_begin_catch',
        lambda s:'GuardD' in s.get('callee', ''))
    assert findings['clang_cfg_unwind_bypasses_destructor']
    assert not findings['llvm_unwind_bypasses_destructor']
    windows_ast = read_function(root / 'windows-ast.out', 'cleanup')
    windows_llvm = read_function(root / 'windows-llvm-read.out', 'cleanup')
    findings['windows_clang_cfg_unwind_bypasses_destructor'] = bypasses_cleanup(
        windows_ast, lambda s:s.get('callee') == 'may_throw',
        lambda s:s.get('text') == 'event(4)', lambda s:s.get('kind') == 'ImplicitDtor')
    findings['windows_llvm_unwind_bypasses_destructor'] = bypasses_cleanup(
        windows_llvm, lambda s:'may_throw' in s.get('callee', ''),
        lambda s:s.get('kind') == 'catchpad', lambda s:'??1Guard' in s.get('callee', ''))
    assert findings['windows_clang_cfg_unwind_bypasses_destructor']
    assert not findings['windows_llvm_unwind_bypasses_destructor']
    for compiler in ['clang++-21', 'g++']:
        observed = (root / ('cleanup-runtime-' + compiler + '.out')).read_text()
        assert observed == '132\n124\n'
    findings['cleanup_native_normal_trace'] = [1,3,2]
    findings['cleanup_native_exception_trace'] = [1,2,4]
    for mode in ['dataflow', 'dataflow-low-budget']:
        functions = json.loads((root / ('clang-' + mode + '.out')).read_text())['functions']
        assert all(f.get('framework_error') == 'Can only analyze C++' for f in functions)
    findings['clang_flow_sensitive_native_c'] = 'rejected: Can only analyze C++'
    functions = json.loads((aux / 'cpp-dataflow-low-budget.out').read_text())['functions']
    assert all(f.get('framework_error') == 'maximum number of blocks processed' for f in functions)
    findings['clang_flow_sensitive_cpp_budget'] = 'explicit incomplete error for every selected function'
    growth = {}
    for size in [64,65536,16777216]:
        growth[str(size)] = stats((aux / ('size-' + str(size) + '-dda.out')).read_text())
    assert growth['64'] == growth['65536'] == growth['16777216']
    findings['svf_buffer_growth'] = growth
    measurements = json.loads((aux / 'measurements.json').read_text())
    findings['svf_buffer_growth_peak_mib'] = {
        r['name']:round(r['max_rss_kib']/1024, 2)
        for r in measurements if r['name'].endswith('-dda')}
    missing = (root / 'external-svf-dda.out').read_text()
    complete = (args.external / 'complete-source.out').read_text()
    blackhole = (args.external / 'missing-with-blackhole.out').read_text()
    assert '[<increment>' in missing and '[<decrement>' not in missing
    assert '[<decrement>' in complete and '[<increment>' not in complete
    assert '[<increment>' in blackhole and '[<decrement>' not in blackhole
    findings['external_callback_write'] = {
        'missing_source_targets': ['increment'], 'complete_source_targets': ['decrement'],
        'blackhole_option_repairs_missing_write': False,
        'native_witness': 'exit 0 only when decrement executes',
        'catalog_coverage_requirement': 'missing source or unvalidated contract keeps target domain open'}
    findings['svf_cutoffs'] = {}
    for unit in ['dispatch','external','php-array','php-invoke']:
        normal = stats((root / (unit + '-svf-dda.out')).read_text())
        low = stats((root / (unit + '-svf-dda-low-budget.out')).read_text())
        assert normal['NumOfOOBQuery'] == ['0']
        assert low['NumOfOOBQuery'] == low['NumOfQuery']
        findings['svf_cutoffs'][unit] = {'normal': normal, 'budget_1': low}
    determinism = {}
    for unit in ['dispatch','cleanup','external','platform','php-array','php-type',
                 'php-session','php-session-user','php-invoke','php-streams']:
        for backend in ['ast','llvm-read']:
            hashes = [hashlib.sha256((root / f'{unit}-{backend}-{i}.out').read_bytes()).hexdigest() for i in range(3)]
            assert len(set(hashes)) == 1, (unit, backend, hashes)
            determinism[unit + '-' + backend] = hashes[0]
    findings['identical_sequential_exports'] = determinism
    findings['scope'] = 'Representation and pointer prerequisites only; no closed PHP behavior summary or Phase F gate is certified.'
    args.output.write_text(json.dumps(findings,indent=2) + '\n')
    print('All comparison evidence checks passed; documented backend limitations reproduced.')


if __name__ == '__main__':
    main()
