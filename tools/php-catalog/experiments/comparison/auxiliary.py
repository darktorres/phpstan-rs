#!/usr/bin/env python3
"""Reproduce growth, incomplete-source, header, and framework experiments."""
import argparse
import json
import shlex
import subprocess
from pathlib import Path

from run import HERE, Runner, digest


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--results', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--probe', type=Path, required=True)
    parser.add_argument('--svf', type=Path, required=True)
    parser.add_argument('--php', type=Path, required=True)
    parser.add_argument('--historical', type=Path)
    parser.add_argument('--large-growth', action='store_true',
                        help='Also measure the 64 MiB buffer (about 2 GiB in stock SVF).')
    args = parser.parse_args()
    root, probe, svf = args.output.resolve(), args.probe.resolve(), args.svf.resolve()
    aux = Runner(root / 'aux')
    names = 'scalar,array_callback,by_reference,exception_cleanup,owner_dispatch,retain,trigger,copy_payload,copied_dispatch,unknown_copy,ordered_dispatch'
    for mode in ['dataflow', 'dataflow-low-budget']:
        aux.run('cpp-' + mode, [probe, mode, names, HERE / 'dispatch.c', '--', '-x', 'c++', '-std=c++17'])
    for compiler in ['clang-21', 'gcc']:
        binary = aux.output / ('external-runtime-' + compiler)
        if aux.run('external-build-' + compiler, [compiler, HERE / 'external.c', HERE / 'external_impl.c', '-o', binary]):
            aux.run('external-runtime-' + compiler, [binary])
    for size in [64, 65536, 16777216] + ([67108864] if args.large_growth else []):
        name = 'size-' + str(size)
        ir, bc = aux.output / (name + '.ll'), aux.output / (name + '.bc')
        entries = 'copy_payload,unknown_copy,copied_dispatch'
        aux.run(name + '-ast', [probe, 'ast', entries, HERE / 'dispatch.c', '--', '-DPAYLOAD_BYTES=' + str(size)])
        aux.run(name + '-emit', ['clang-21', '-O0', '-Xclang', '-disable-O0-optnone', '-g',
            '-fno-discard-value-names', '-S', '-emit-llvm', '-DPAYLOAD_BYTES=' + str(size), HERE / 'dispatch.c', '-o', ir])
        aux.run(name + '-ir', [probe, 'ir', entries, ir])
        aux.run(name + '-promote', ['opt-21', '-passes=mem2reg', ir, '-o', bc])
        aux.run(name + '-dda', [svf / 'dvf', '-dfs', '-query=funptr', '-stat', '-print-query-pts', bc])
    external = Runner(root / 'external-comparison')
    impl, linked = external.output / 'impl.bc', external.output / 'linked.bc'
    external.run('impl-emit', ['clang-21', '-O0', '-Xclang', '-disable-O0-optnone', '-g',
        '-fno-discard-value-names', '-c', '-emit-llvm', HERE / 'external_impl.c', '-o', impl])
    external.run('external-link', ['llvm-link-21', args.results / 'external-promoted.bc', impl, '-o', linked])
    for name, flags, source in [('missing-with-blackhole', ['-blk'], args.results / 'external-promoted.bc'),
                                ('complete-source', [], linked)]:
        external.run(name, [svf / 'dvf', '-dfs', '-query=funptr', '-stat', '-print-query-pts', *flags, source])
    header = Runner(root / 'headers')
    native = header.output / 'unit.c'
    include = header.output / 'layout.h'
    native.write_text('#include "layout.h"\nvoid owner_dispatch(Owner *o) { o->callback(o->argument); }\n')
    revisions = []
    for revision, prefix in [(0, ''), (1, 'char prefix[16]; ' )]:
        include.write_text('typedef void (*Callback)(int *);\ntypedef struct Owner { ' +
                          prefix + 'Callback callback; int *argument; } Owner;\n')
        header.run('header-' + str(revision), [probe, 'ast', 'owner_dispatch', native])
        exported = json.loads((header.output / ('header-' + str(revision) + '.out')).read_text())
        owner = next(layout for layout in exported['layouts'] if layout['name'] == 'Owner')
        revisions.append(dict(source_sha256=digest(native), header_sha256=digest(include), layout=owner))
    assert revisions[0]['source_sha256'] == revisions[1]['source_sha256']
    assert revisions[0]['header_sha256'] != revisions[1]['header_sha256']
    assert revisions[0]['layout'] != revisions[1]['layout']
    (header.output / 'invalidation.json').write_text(json.dumps(revisions, indent=2) + '\n')
    # Inventory every actually read system/project header for the locked slice.
    inputs = json.loads((args.results / 'inputs.json').read_text())
    dependencies = {}
    for relative, recipe in inputs['actual_compile_recipes'].items():
        command = ['clang-21', '-M', *recipe['frontend_flags'], args.php / relative]
        deptext = subprocess.check_output(list(map(str, command)), cwd=args.php, text=True,
                                         stderr=subprocess.DEVNULL)
        paths = shlex.split(deptext.split(':', 1)[1].replace('\\\n', ' '))
        dependencies[relative] = {str(Path(path).resolve() if Path(path).is_absolute() else
            (args.php / path).resolve()): digest(Path(path) if Path(path).is_absolute() else args.php / path)
            for path in paths}
    (root / 'dependencies.json').write_text(json.dumps(dependencies, indent=2) + '\n')
    if args.historical:
        historical = Runner(root / 'historical')
        base = args.historical.resolve()
        exported_root = base / 'root'
        commands = json.loads((base / 'compile_commands.json').read_text())
        for name, relative, entries in [('array', 'ext/standard/array.c', 'zif_array_map,zif_count'),
                                       ('invoke', 'Zend/zend_execute_API.c', 'zend_call_function')]:
            recipe = next(e for e in commands if e['file'] == '/usr/src/php/' + relative)
            flags = []
            for flag in recipe['arguments'][1:recipe['arguments'].index('-c')]:
                if flag.startswith('-I'):
                    path = Path(flag[2:])
                    if not path.is_absolute(): path = Path(recipe['directory']) / path
                    flags.append('-I' + str(exported_root / str(path).lstrip('/')))
                elif not flag.startswith(('-O', '-W')): flags.append(flag)
            flags += ['-ivfsoverlay', str(base / 'clang-overlay.yaml'), '-nostdinc',
                      '-isystem', '/usr/lib/llvm-21/lib/clang/21/include',
                      '-isystem', str(exported_root / 'usr/include/x86_64-linux-gnu'),
                      '-isystem', str(exported_root / 'usr/include')]
            source, cwd = exported_root / 'usr/src/php' / relative, exported_root / 'usr/src/php'
            historical.run('php84-' + name + '-ast', [probe, 'ast', entries, source, '--', *flags], cwd=cwd)
            ir = historical.output / (name + '.ll')
            historical.run('php84-' + name + '-emit', ['clang-21', '-O0', '-g', '-fno-discard-value-names',
                '-S', '-emit-llvm', *flags, source, '-o', ir], cwd=cwd)
            if ir.exists(): historical.run('php84-' + name + '-read', [probe, 'ir', entries, ir])
        (historical.output / 'inputs.json').write_text(json.dumps({
            'dist_sha256': digest(base / 'dist.tar'),
            'compile_commands_sha256': digest(base / 'compile_commands.json'),
            'overlay_sha256': digest(base / 'clang-overlay.yaml'),
            'scope': 'Clang replay of two historical GCC units does not certify GCC equivalence'
        }, indent=2) + '\n')


if __name__ == '__main__':
    main()
