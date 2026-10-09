#!/usr/bin/env python3
"""Serial, enforcing evaluation of reuse adaptations and complementary tools."""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import time

from evaluation import EnforcedRunner
from run import HERE, digest


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


def complementary(cache, runner):
    environment = dict(os.environ, LD_LIBRARY_PATH=str(cache/'deps/usr/lib'))
    cbmc = cache / 'deps/usr/bin/cbmc'
    for name, source, flags in [
        ('cbmc-contracts', 'contracts.c', ['--unwind', '4']),
        ('cbmc-invalid-composition', 'contracts.c', ['--unwind', '4', '-DINVALID_COMPOSITION']),
        ('cbmc-low-budget', 'loop_contract.c', ['--unwind', '2']),
    ]:
        runner.run(name, [cbmc, HERE/source, '--unwinding-assertions', '--bounds-check', '--pointer-check',
            '--json-ui', *flags], timeout=300, env=environment)
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
