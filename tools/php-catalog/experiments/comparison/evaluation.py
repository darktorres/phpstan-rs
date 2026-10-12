#!/usr/bin/env python3
"""Whole-link, enforcing evaluation. Capability failure is retained as evidence."""
import argparse
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys

from run import HERE, Runner, digest, recorded, svf_command

REPOSITORY = HERE.parents[3]


class EnforcedRunner(Runner):
    # This worker's physical aggregate cap is enforced by the kernel. A virtual
    # address-space cap would reject large reserved mappings prematurely.
    child_limits = None
    stop_on_sampled_rss = False
    def run(self, name, command, cwd=None, timeout=180, env=None):
        proof = self.output / (name + '.cgroup.json')
        result = super().run(name, ['systemd-run', '--user', '--scope', '--quiet',
            '-p', 'MemoryMax=10737418240', '-p', 'MemorySwapMax=0',
            sys.executable, HERE / 'worker.py', proof, *map(str, command)],
            cwd=cwd, timeout=timeout, env=env)
        record = self.measurements[-1]
        record['job_command'] = recorded(list(map(str, command)))
        if proof.exists():
            record['resource_proof'] = recorded(json.loads(proof.read_text()))
        else:
            record['resource_proof'] = {'aggregate_hard_limit_enforced': False}
        (self.output / 'measurements.json').write_text(json.dumps(self.measurements, indent=2) + '\n')
        if not record['resource_proof']['aggregate_hard_limit_enforced']:
            raise RuntimeError('Cannot certify resource enforcement for ' + name)
        if not result:
            unit = Path(record['resource_proof']['cgroup']).name
            journal = subprocess.run(['journalctl', '--user', '--unit', unit, '--no-pager', '-o', 'json'],
                text=True, capture_output=True)
            messages = [json.loads(line).get('MESSAGE', '') for line in journal.stdout.splitlines() if line.startswith('{')]
            record['resource_proof']['systemd_messages'] = messages
            record['resource_proof']['kernel_oom_kill'] = any('kernel OOM killer' in line for line in messages)
            (self.output / 'measurements.json').write_text(json.dumps(self.measurements, indent=2) + '\n')
        return result


def recipes(php):
    text = subprocess.check_output(['make', '-n', '-B', 'sapi/cli/php'], cwd=php, text=True)
    compilations = {}
    link = None
    for line in text.replace('\\\n', ' ').splitlines():
        if not (' -c ' in line or ' -o sapi/cli/php' in line):
            continue
        words = shlex.split(line)
        compiler = next((i for i, x in enumerate(words) if x in ['clang-21', 'gcc', 'cc']), None)
        if compiler is None:
            continue
        command = words[compiler:]
        if '-c' in command:
            # The source keeps the spelling make used, so the bitcode names it relative to the tree.
            source_arg = command[command.index('-c') + 1]
            target = Path(command[command.index('-o') + 1])
            source = Path(source_arg) if Path(source_arg).is_absolute() else php / source_arg
            compilations[str(target)] = dict(source=str(source), source_arg=source_arg, command=command)
            if target.suffix == '.lo':
                compilations[str(target.with_suffix('.o'))] = compilations[str(target)]
        elif 'sapi/cli/php' in command:
            link = command
    if link is None:
        raise RuntimeError('No actual CLI link command found')
    linked = [x for x in link if x.endswith(('.lo', '.o'))]
    if not linked:
        raise RuntimeError('CLI link contained no object files')
    missing = [x for x in linked if x not in compilations]
    if missing:
        raise RuntimeError('Missing compile recipes: ' + str(missing))
    return compilations, linked, link


def whole_link(args):
    php = args.php.resolve()
    runner = EnforcedRunner(args.output)
    compiles, linked, link = recipes(php)
    locked = json.loads((HERE / 'results' / 'lock.json').read_text())['php_commit']
    head = subprocess.check_output(['git', '-C', str(php), 'rev-parse', 'HEAD'], text=True).strip()
    if head != locked:
        raise RuntimeError(f'php tree is at {head}, not the locked {locked}')
    manifest = dict(php_commit=head,
                    link_command=link, external_link_flags=[x for x in link if x.startswith('-l')],
                    objects=[], scope='Actual minimal NTS CLI link; external libraries remain declared boundaries')
    modules = []
    roots = 'zif_array_map,zif_array_walk,zif_array_filter,zif_usort,zif_count,zif_strlen,zif_session_start,zif_session_set_save_handler,ps_call_handler,ps_open_user,ps_read_user,ps_write_user,ps_close_user,zend_call_function,_php_stream_open_wrapper_ex,_php_stream_free'
    for i, target in enumerate(linked):
        recipe = compiles[target]
        source = Path(recipe['source'])
        name = f'unit-{i:03d}'
        if source.suffix.lower() in ['.s', '.asm']:
            manifest['objects'].append(dict(object=target, source=str(source), source_sha256=digest(source),
                imported=False, reason='native-assembly-boundary', native_compile_command=recipe['command']))
            continue
        bc = runner.output / (name + '.bc')
        flags = recipe['command'][1:recipe['command'].index('-c')]
        # Baseline is O0. Keep semantic frontend flags, replace debug verbosity only.
        flags = [x for x in flags if not x.startswith('-O') and x != '-g']
        # Build-time prefix maps name the build root; these name this tree and this repository.
        flags = [x for x in flags if not x.startswith(('-ffile-prefix-map=', '-fdebug-prefix-map='))]
        flags += [f'-ffile-prefix-map={php}=.', f'-ffile-prefix-map={REPOSITORY}=.']
        command = ['clang-21', '-O0', '-gline-tables-only', '-Xclang', '-disable-O0-optnone',
                   '-emit-llvm', '-c', *flags, recipe['source_arg'], '-o', bc]
        passed = runner.run(name + '-emit', command, cwd=php, timeout=600)
        record = dict(object=target, source=str(source), source_sha256=digest(source),
                      native_compile_command=recipe['command'], bitcode_command=list(map(str, command)),
                      imported=passed, module_sha256=digest(bc) if passed else None)
        manifest['objects'].append(record)
        (runner.output / 'link-manifest.json').write_text(json.dumps(recorded(manifest), indent=2) + '\n')
        if not passed:
            continue
        modules.append(bc)
        if args.probe:
            runner.run(name + '-index', [args.probe, 'ir', roots, bc], cwd=php, timeout=600)
    manifest['all_link_objects_imported'] = len(modules) == len(linked)
    manifest['all_c_link_objects_imported'] = all(x['imported'] or x.get('reason') == 'native-assembly-boundary' for x in manifest['objects'])
    whole = runner.output / 'php-cli.bc'
    if manifest['all_c_link_objects_imported']:
        manifest['llvm_link_completed'] = runner.run('whole-link', ['llvm-link-21', *modules, '-o', whole], timeout=600)
        if manifest['llvm_link_completed']:
            manifest['linked_bitcode_sha256'] = digest(whole)
            runner.run('whole-verify', ['opt-21', '-passes=verify', '-disable-output', whole], timeout=600)
            if args.probe:
                runner.run('whole-index', [args.probe, 'ir', roots, whole], timeout=600)
            if args.svf:
                promoted = runner.output / 'php-cli-promoted.bc'
                if runner.run('whole-promote', ['opt-21', '-passes=mem2reg', whole, '-o', promoted], timeout=600):
                    runner.run('whole-svf-dda', svf_command(args.svf.resolve() / 'dvf', '-dfs', '-query=funptr', '-stat',
                        '-print-query-pts', promoted), timeout=args.analysis_timeout)
    (runner.output / 'link-manifest.json').write_text(json.dumps(recorded(manifest), indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--php', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--probe', type=Path)
    parser.add_argument('--svf', type=Path)
    parser.add_argument('--analysis-timeout', type=int, default=1800)
    args = parser.parse_args()
    whole_link(args)


if __name__ == '__main__':
    main()
