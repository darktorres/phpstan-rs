#!/usr/bin/env python3
"""Measured, bounded compiler/framework comparison. Requires configured php-src.

No dependency acquisition, publication, or production catalog generation occurs.
Each command has durable stdout/stderr, GNU time measurements, and sampled group
RSS. Samples are observations, not an enforceable aggregate memory guarantee.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import signal
import shlex
import subprocess
import time

HERE = Path(__file__).resolve().parent
LIMIT = 10 * 1024**3
REPOSITORY_ROOT = HERE.parents[3]
CACHE_ROOT = Path.home() / '.cache' / 'php-catalog-evaluation'
# Records name the evaluation cache and the checkout by these placeholders, so a record
# made under one location reads the same under another; each reader resolves them.
PLACEHOLDERS = (('<cache>', CACHE_ROOT), ('<repo>', REPOSITORY_ROOT))


def record_text(value):
    text = str(value)
    for token, root in PLACEHOLDERS:
        prefix = str(root)
        if text == prefix or text.startswith(prefix + '/'):
            return token + text[len(prefix):]
    return text


def resolve_record(value):
    text = str(value)
    for token, root in PLACEHOLDERS:
        if text == token or text.startswith(token + '/'):
            return str(root) + text[len(token):]
    return text


def recorded(value):
    if isinstance(value, dict):
        return {key: recorded(item) for key, item in value.items()}
    if isinstance(value, list):
        return [recorded(item) for item in value]
    if isinstance(value, (str, Path)):
        return record_text(value)
    return value


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def memory_limit():
    resource.setrlimit(resource.RLIMIT_AS, (LIMIT, LIMIT))


def group_rss(group):
    total = 0
    for stat in Path('/proc').glob('[0-9]*/stat'):
        try:
            parts = stat.read_text().rsplit(')', 1)[1].split()
            if int(parts[2]) == group:
                total += int(parts[21]) * os.sysconf('SC_PAGE_SIZE')
        except (OSError, ValueError, IndexError):
            pass
    return total


class Runner:
    child_limits = staticmethod(memory_limit)
    stop_on_sampled_rss = True
    def __init__(self, output):
        self.output = output.resolve()
        self.output.mkdir(parents=True, exist_ok=True)
        self.measurements = []

    def run(self, name, command, cwd=None, timeout=180, env=None):
        stdout = self.output / (name + '.out')
        stderr = self.output / (name + '.err')
        timing = self.output / (name + '.time.json')
        wrapped = ['/usr/bin/time', '-q', '-f',
                   '{"max_rss_kib":%M,"user_seconds":%U,"system_seconds":%S,"exit_status":%x}',
                   '-o', str(timing), *map(str, command)]
        start = time.monotonic()
        sampled_peak = 0
        stopped = None
        with stdout.open('w') as out, stderr.open('w') as err:
            process = subprocess.Popen(wrapped, cwd=cwd, stdout=out, stderr=err,
                                       env=env, start_new_session=True,
                                       preexec_fn=self.child_limits)
            try:
                while process.poll() is None:
                    sampled_peak = max(sampled_peak, group_rss(process.pid))
                    if self.stop_on_sampled_rss and sampled_peak > LIMIT:
                        stopped = 'sampled-group-memory-limit'
                    elif time.monotonic() - start > timeout:
                        stopped = 'timeout'
                    if stopped:
                        os.killpg(process.pid, signal.SIGKILL)
                        break
                    time.sleep(0.025)
                process.wait()
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
        measured = {}
        if timing.exists():
            for line in timing.read_text().splitlines():
                if line.startswith('{'):
                    measured = json.loads(line)
        record = dict(name=name, command=[record_text(x) for x in command], cwd=record_text(cwd or Path.cwd()),
                      elapsed_seconds=round(time.monotonic() - start, 4),
                      sampled_group_peak_bytes=sampled_peak,
                      stdout_path=record_text(stdout.resolve()), stdout_sha256=digest(stdout),
                      stderr_path=record_text(stderr.resolve()), stderr_sha256=digest(stderr),
                      exit_code=process.returncode, stopped=stopped, **measured)
        self.measurements.append(record)
        (self.output / 'measurements.json').write_text(json.dumps(self.measurements, indent=2) + '\n')
        print(f'{name}: exit={process.returncode}, {record["elapsed_seconds"]}s, '
              f'GNU peak={measured.get("max_rss_kib", 0)/1024:.1f}MiB', flush=True)
        return process.returncode == 0


PHP_UNITS = {
    'array': ('ext/standard/array.c', 'zif_array_map,zif_array_filter,zif_array_walk,zif_usort,zif_count'),
    'type': ('Zend/zend_builtin_functions.c', 'zif_strlen'),
    'session': ('ext/session/session.c', 'zif_session_start,zif_session_set_save_handler'),
    'session-user': ('ext/session/mod_user.c', 'ps_call_handler,ps_open_user,ps_read_user,ps_write_user,ps_close_user'),
    'invoke': ('Zend/zend_execute_API.c', 'zend_call_function,zend_release_fcall_info_cache'),
    'streams': ('main/streams/streams.c', '_php_stream_open_wrapper_ex,_php_stream_free'),
}


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--php', type=Path, required=True)
    parser.add_argument('--probe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--svf', type=Path)
    parser.add_argument('--call-php', type=Path)
    args = parser.parse_args()
    php, probe = args.php.resolve(), args.probe.resolve()
    runner = Runner(args.output)
    # Replay each Makefile compile recipe's frontend flags rather than inventing
    # analysis-only semantics flags (NDEBUG and ZEND_SIGNALS matter here).
    compile_recipes = {}
    def build_flags(path):
        target = str(Path(path).with_suffix('.lo'))
        commands = subprocess.check_output(['make', '-n', '-B', target], cwd=php, text=True)
        for line in commands.replace('\\\n', ' ').splitlines():
            if 'clang-21' not in line or ' -c ' not in line:
                continue
            words = shlex.split(line)
            if 'clang-21' not in words or '-c' not in words:
                continue
            compiler_at, source_at = words.index('clang-21'), words.index('-c')
            if words[source_at + 1] != str(php / path):
                continue
            flags = [x for x in words[compiler_at + 1:source_at] if not x.startswith('-O') and x != '-g']
            compile_recipes[path] = dict(command=words[compiler_at:], frontend_flags=flags)
            return flags
        raise RuntimeError('No actual compile recipe found for ' + path)
    fixture_names = 'scalar,array_callback,by_reference,exception_cleanup,owner_dispatch,retain,trigger,copy_payload,copied_dispatch,unknown_copy,ordered_dispatch'
    units = [('dispatch', HERE / 'dispatch.c', fixture_names, [], 'clang-21'),
             ('cleanup', HERE / 'cleanup.cpp', 'cleanup', ['-std=c++17'], 'clang++-21'),
             ('external', HERE / 'external.c', 'external_write_entry', [], 'clang-21'),
             ('platform', HERE / 'platform.c', '*', [], 'clang-21')]
    units += [('php-' + key, php / path, entries, build_flags(path), 'clang-21')
              for key, (path, entries) in PHP_UNITS.items()]
    summaries = []
    for name, source, entries, flags, compiler in units:
        for repeat in range(3):
            ast_name = f'{name}-ast-{repeat}'
            runner.run(ast_name, [probe, 'ast', entries, source, '--', *flags], cwd=php)
            ir_file = runner.output / f'{name}-{repeat}.ll'
            runner.run(f'{name}-llvm-emit-{repeat}',
                       [compiler, '-O0', '-Xclang', '-disable-O0-optnone', '-g', '-fno-discard-value-names', '-S', '-emit-llvm',
                        *flags, source, '-o', ir_file], cwd=php)
            if ir_file.exists():
                ir_name = f'{name}-llvm-read-{repeat}'
                runner.run(ir_name, [probe, 'ir', entries, ir_file], cwd=php)
                if repeat == 0:
                    for backend, result in [('ast', runner.output / (ast_name + '.out')),
                                            ('llvm', runner.output / (ir_name + '.out'))]:
                        try:
                            data = json.loads(result.read_text())
                            summaries.append(dict(unit=name, backend=backend,
                                definitions=data['definitions'], source_sha256=digest(source),
                                artifact_bytes=result.stat().st_size,
                                functions=[{k:v for k,v in f.items() if k != 'blocks'} for f in data['functions']],
                                layouts=data.get('layouts', [])))
                        except (ValueError, KeyError):
                            pass
        # Native GCC comparison uses actual gimple dumps, not a hand-written lowering.
        gcc = 'g++' if source.suffix == '.cpp' else 'gcc'
        runner.run(name + '-gimple', [gcc, '-O0', '-g', '-fdump-tree-cfg',
            '-fdump-tree-gimple', '-fdump-tree-eh', *flags, '-c', source, '-o', runner.output / (name + '.o')], cwd=php)
        runner.run(name + '-cir', [compiler, '-fclangir', '-Xclang', '-emit-cir',
            '-S', *flags, source, '-o', runner.output / (name + '.cir')], cwd=php)
    # The stock diagnostic engine is audited as a reuse candidate, not treated as a summary producer.
    for config in ['default', 'tiny-budget']:
        flags = [] if config == 'default' else ['-Xclang', '-analyzer-max-loop', '-Xclang', '1',
            '-Xclang', '-analyzer-config', '-Xclang', 'max-nodes=20']
        runner.run('clang-analyzer-' + config, ['clang-21', '--analyze', '-Xclang',
            '-analyzer-checker=debug.Stats', *flags, HERE / 'dispatch.c', '-o', os.devnull])
    for mode in ['dataflow', 'dataflow-low-budget']:
        runner.run('clang-' + mode, [probe, mode, fixture_names, HERE / 'dispatch.c'])
    windows_ir = runner.output / 'cleanup-windows.ll'
    windows_flags = ['--target=x86_64-pc-windows-msvc', '-fexceptions', '-fcxx-exceptions', '-std=c++17']
    runner.run('windows-llvm-emit', ['clang++-21', '-O0', '-g', '-fno-discard-value-names',
        '-S', '-emit-llvm', *windows_flags, HERE / 'cleanup.cpp', '-o', windows_ir])
    if windows_ir.exists():
        runner.run('windows-llvm-read', [probe, 'ir', 'cleanup', windows_ir])
    runner.run('windows-ast', [probe, 'ast', 'cleanup', HERE / 'cleanup.cpp', '--', *windows_flags])
    for compiler in ['clang++-21', 'g++']:
        executable = runner.output / ('cleanup-runtime-' + compiler)
        if runner.run('cleanup-build-' + compiler, [compiler, HERE / 'cleanup.cpp',
                      HERE / 'cleanup_runtime.cpp', '-o', executable]):
            runner.run('cleanup-runtime-' + compiler, [executable])
    runner.run('host-runtime', ['php', '-n', HERE / 'runtime.php'])
    for vm in ['normal', 'call']:
        runtime = (args.call_php.resolve() if vm == 'call' and args.call_php else
                   php / ('sapi/cli/php' if vm == 'normal' else 'build-call/sapi/cli/php'))
        if runtime.exists():
            runner.run('php-runtime-' + vm, [runtime, '-n', HERE / 'runtime.php'])
    if args.svf:
        svf = args.svf.resolve()
        environment = dict(os.environ)
        # Configuration embeds its Z3 rpath; no environmental substitute for a different Z3 is used.
        for unit in ['dispatch', 'external', 'php-array', 'php-invoke']:
            ir_file = runner.output / (unit + '-0.ll')
            if not ir_file.exists():
                continue
            promoted = runner.output / (unit + '-promoted.bc')
            runner.run(unit + '-promote', ['opt-21', '-passes=mem2reg', ir_file, '-o', promoted])
            for kind, flags in [('andersen', ['-ander', '-stat', '-print-pts']),
                                ('dda', ['-dfs', '-query=funptr', '-stat', '-print-query-pts']),
                                ('dda-low-budget', ['-dfs', '-query=funptr', '-stat', '-print-query-pts', '-flow-bg=1'])]:
                executable = svf / ('wpa' if kind == 'andersen' else 'dvf')
                if executable.exists():
                    runner.run(unit + '-svf-' + kind,
                               [executable, *flags, promoted], timeout=180, env=environment)
    (runner.output / 'representations.json').write_text(json.dumps(summaries, indent=2) + '\n')
    versions = {}
    for tool in ['clang-21', 'llvm-config-21', 'gcc', 'php']:
        command = [tool, '-v'] if tool == 'php' else [tool, '--version']
        versions[tool] = subprocess.check_output(command, text=True, stderr=subprocess.STDOUT).splitlines()[0]
    lock = dict(php_commit=subprocess.check_output(['git', '-C', str(php), 'rev-parse', 'HEAD'], text=True).strip(),
                php_config_sha256=digest(php / 'main/php_config.h'), versions=versions,
                actual_compile_recipes=compile_recipes,
                vm_header_sha256=digest(php / 'Zend/zend_vm_execute.h'),
                fixture_sha256={p.name:digest(p) for p in HERE.iterdir() if p.is_file()},
                probe_sha256=digest(probe),
                resource_policy={'per_process_virtual_limit_bytes':LIMIT, 'sampled_group_rss_stop_bytes':LIMIT,
                                 'aggregate_hard_limit_enforced':False, 'samples_seconds':0.025},
                repetitions='3 sequential warm-filesystem repetitions; not cold cache or warm summary cache')
    (runner.output / 'inputs.json').write_text(json.dumps(lock, indent=2) + '\n')


if __name__ == '__main__':
    main()
