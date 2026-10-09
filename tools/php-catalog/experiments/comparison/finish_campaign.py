#!/usr/bin/env python3
"""Continue the serial campaign with runtime witnesses and native producers."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

from evaluation import EnforcedRunner, recipes
from run import HERE, digest


def instrumented(cache, runner):
    whole = cache/'whole-clang/php-cli.bc'
    helper = runner.output/'instrument'
    for name in ['instrument', 'closure']:
        command = ['clang++-21', '-std=c++17', '-O1', '-I/usr/lib/llvm-21/include', HERE/(name+'.cpp'),
            '-L/usr/lib/llvm-21/lib', '-lLLVM-21', '-o', runner.output/name]
        assert runner.run(name+'-build', command, timeout=300)
    modified, sites = runner.output/'php-instrumented.bc', runner.output/'sites.json'
    assert runner.run('instrument-whole', [helper, whole, modified, sites], timeout=600)
    assert runner.run('instrument-verify', ['opt-21', '-passes=verify', '-disable-output', modified], timeout=600)
    manifest = json.loads((cache/'whole-clang/link-manifest.json').read_text())
    php = Path('/tmp/php-catalog-comparison/php-src')
    asm = [php/x['object'] for x in manifest['objects'] if x.get('reason') == 'native-assembly-boundary']
    binary = runner.output/'php-instrumented'
    assert runner.run('instrument-link', ['clang-21', '-O0', '-no-pie', '-Wl,--export-dynamic', modified,
        HERE/'target_runtime.c', *asm, '-lm', '-o', binary], timeout=600)
    runner.run('instrumented-runtime', [binary, '-n', HERE/'runtime.php'], timeout=180)
    runner.run('instrument-symbols', ['nm', '-n', binary])
    symbols = {}
    for line in (runner.output/'instrument-symbols.out').read_text().splitlines():
        words = line.split()
        if len(words) == 3 and words[1] in ['t','T','W','w']:
            symbols[int(words[0],16)] = words[2]
    metadata = json.loads(sites.read_text())
    lookup = {site['id']:site for site in metadata['sites']}
    observations = {}
    for line in (runner.output/'instrumented-runtime.err').read_text().splitlines():
        words = line.split()
        if len(words) != 3 or words[0] != 'PTA': continue
        site, address = int(words[1],16), int(words[2],16)
        target = symbols.get(address, '<external-or-unresolved-address>')
        observations.setdefault(site, set()).add(target)
    (runner.output/'target-witnesses.json').write_text(json.dumps(dict(
        binary_sha256=digest(binary), uninstrumented_module_sha256=digest(whole),
        instrumented_module_sha256=digest(modified), indirect_sites=len(lookup),
        observed_sites=len(observations), targets=[dict(**lookup[site], targets=sorted(targets))
            for site, targets in sorted(observations.items())],
        evidence='positive witnesses; unobserved sites remain open'), indent=2)+'\n')
    roots='zif_array_map,zif_array_walk,zif_array_filter,zif_usort,zif_count,zif_strlen,zif_session_start,zif_session_set_save_handler,ps_call_handler,zend_call_function,_php_stream_open_wrapper_ex,_php_stream_free'
    runner.run('native-closure-audit', [runner.output/'closure', whole, roots], timeout=600)


def gcc_profile(cache, runner):
    php = cache/'php-src-gcc'
    environment = dict(os.environ)
    environment['PATH'] = '/tmp/php-catalog-comparison/deps/usr/bin:' + environment['PATH']
    environment['BISON_PKGDATADIR'] = '/tmp/php-catalog-comparison/deps/usr/share/bison'
    assert runner.run('gcc-buildconf', ['./buildconf', '--force'], cwd=php, env=environment)
    environment.update(CC='gcc', CXX='g++', CFLAGS='-O0 -g')
    assert runner.run('gcc-configure', ['./configure', '--disable-all', '--disable-cgi', '--disable-phpdbg',
        '--without-pear', '--enable-cli', '--enable-session'], cwd=php, env=environment, timeout=600)
    assert runner.run('gcc-build', ['make', '-j2'], cwd=php, env=environment, timeout=1800)
    assert runner.run('gcc-runtime', [php/'sapi/cli/php', '-n', HERE/'runtime.php'])
    commands, linked, link = recipes(php)
    manifest = dict(link_command=link, php_config_sha256=digest(php/'main/php_config.h'),
        vm_header_sha256=digest(php/'Zend/zend_vm_execute.h'), binary_sha256=digest(php/'sapi/cli/php'),
        compiler=subprocess.check_output(['gcc','--version'],text=True).splitlines()[0], objects=[])
    for i, target in enumerate(linked):
        recipe=commands[target]; source=Path(recipe['source'])
        if source.suffix.lower() in ['.s','.asm']:
            manifest['objects'].append(dict(object=target, imported=False, reason='native-assembly-boundary'))
            continue
        flags=recipe['command'][1:recipe['command'].index('-c')]
        output=runner.output/('gcc-'+str(i)+'.o')
        success=runner.run('gcc-'+str(i)+'-gimple', ['gcc','-fdump-tree-gimple','-fdump-tree-cfg','-fdump-tree-eh',
            *flags,'-c',source,'-o',output],cwd=php,timeout=600)
        manifest['objects'].append(dict(object=target,source=str(source),source_sha256=digest(source),
            imported=success,command=recipe['command']))
        (runner.output/'gcc-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')


def cir_profile(cache, runner):
    build=cache/'cir-full-build'
    if not runner.run('cir-build', ['cmake','--build',build,'--target','clang','--parallel','8'],timeout=14400):
        return
    compiler=build/'bin/clang'
    fixture_inputs=[('dispatch',HERE/'dispatch.c',[]),('platform',HERE/'platform.c',[]),
        ('cleanup',HERE/'cleanup.cpp',['-std=c++17']),
        ('cleanup-windows',HERE/'cleanup.cpp',['-std=c++17','--target=x86_64-pc-windows-msvc','-fexceptions','-fcxx-exceptions'])]
    php=Path('/tmp/php-catalog-comparison/php-src')
    commands, linked, _=recipes(php)
    for target in linked:
        recipe=commands[target]; source=Path(recipe['source'])
        if source.suffix.lower() in ['.s','.asm']:continue
        flags=recipe['command'][1:recipe['command'].index('-c')]
        fixture_inputs.append(('php-'+target.replace('/','-'),source,flags))
    results=[]
    for name, source, flags in fixture_inputs:
        result=runner.run(name+'-cir', [compiler,'-fclangir','-S','-Xclang','-emit-cir',*flags,
            source,'-o',runner.output/(name+'.cir')],cwd=php,timeout=600)
        results.append(dict(name=name,imported=result))
        (runner.output/'cir-imports.json').write_text(json.dumps(results,indent=2)+'\n')


def main():
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--cache',type=Path,required=True)
    parser.add_argument('--stage',choices=['all','gcc','cir'],default='all')
    args=parser.parse_args();cache=args.cache.resolve()
    ready=cache/'campaign/completed.json'
    while not ready.exists():time.sleep(1)
    runner=EnforcedRunner(cache/('finish' if args.stage=='all' else 'finish-'+args.stage))
    if args.stage=='gcc':
        gcc_profile(cache,runner)
        return
    if args.stage=='cir':
        cir_profile(cache,runner)
        return
    runner.run('binary-retry', [cache/'python/bin/python', HERE/'binary_cases.py', '--fixture', cache/'campaign/layout-clang-21',
        '--php', '/tmp/php-catalog-comparison/php-src/sapi/cli/php', '--output', runner.output/'binary-results.json'], timeout=300)
    for candidate, binary in [('stock',Path('/tmp/php-catalog-comparison/svf-build/bin/dvf')),
                              ('compact',cache/'campaign/dvf-compact')]:
        for name, flags in [('constants',['-model-consts=true']),
                            ('arrays',['-model-arrays=true']),
                            ('constants-arrays',['-model-consts=true','-model-arrays=true'])]:
            runner.run('layout-'+candidate+'-'+name, [binary,'-dfs','-query=funptr','-stat','-print-query-pts',
                *flags,cache/'campaign/layout_cases-promoted.bc'])
    instrumented(cache,runner)
    gcc_profile(cache,runner)
    cir_profile(cache,runner)
    (runner.output/'completed.json').write_text('{}\n')


if __name__=='__main__':main()
