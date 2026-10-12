#!/usr/bin/env python3
"""Continue the serial campaign with runtime witnesses and native producers."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time

from evaluation import EnforcedRunner, recipes
from campaign import expected_outcome, step_passed
from run import HERE, digest, recorded, svf_command


def require_success(success, name):
    if not success:
        raise RuntimeError(f"campaign step failed: {name}")


def finish_expected_exit_codes(measurements, cir_results):
    cir_by_name = {item['name'] + '-cir': item for item in cir_results}
    if len(cir_by_name) != len(cir_results):
        raise ValueError('CIR outcome manifest contains duplicate translation units')
    cir_runs = {record['name'] for record in measurements if record['name'].endswith('-cir')}
    if cir_runs != set(cir_by_name):
        raise ValueError('CIR outcome manifest does not match executed CIR commands')
    expected = {}
    for record in measurements:
        name = record['name']
        if name not in cir_by_name:
            expected[name] = expected_outcome(name)['exit_codes']
            continue
        result = cir_by_name[name]
        if result.get('stderr_sha256') != record.get('stderr_sha256') or re.fullmatch(
            r'[0-9a-f]{64}', str(result.get('stderr_sha256', ''))
        ) is None:
            raise ValueError(f'{name}: CIR outcome does not bind the producer stderr digest')
        if result.get('imported') is True:
            output_path = result.get('output_path')
            output_sha256 = result.get('output_sha256')
            if (result.get('exit_code') != 0 or result.get('classification') != 'emitted'
                    or not isinstance(output_path, str)
                    or re.fullmatch(r'[0-9a-f]{64}', str(output_sha256)) is None
                    or not Path(output_path).is_file()
                    or digest(Path(output_path)) != output_sha256):
                raise ValueError(f'{name}: successful CIR outcome is malformed')
        elif (result.get('exit_code') != 1
              or result.get('classification') != 'frontend rejected this translation unit; retained as unsupported evidence'
              or not isinstance(result.get('stderr_sha256'), str)):
            raise ValueError(f'{name}: CIR rejection is not an explicitly classified unsupported result')
        expected[name] = [result['exit_code']]
    return expected


def validate_campaign_completion(cache, campaign=None):
    campaign = Path(campaign) if campaign is not None else cache / 'campaign'
    measurements_path = campaign / 'measurements.json'
    marker_path = campaign / 'completed.json'
    measurements = json.loads(measurements_path.read_text())
    marker = json.loads(marker_path.read_text())
    names = [record['name'] for record in measurements]
    if len(set(names)) != len(names):
        raise ValueError('campaign measurements contain duplicate step names')
    if marker.get('schema') != 1 or marker.get('status') != 'complete':
        raise ValueError('campaign completion marker has no successful schema/status')
    if marker.get('step_count') != len(measurements) or marker.get('steps') != names:
        raise ValueError('campaign completion marker does not bind the measured steps')
    expected = {record['name']: expected_outcome(record['name'])['exit_codes'] for record in measurements}
    if marker.get('expected_exit_codes') != expected:
        raise ValueError('campaign completion marker has an unexpected exit-code policy')
    if marker.get('measurements_sha256') != digest(measurements_path):
        raise ValueError('campaign measurement digest differs from completion marker')
    failures = [record['name'] for record in measurements if not step_passed(record)]
    if failures:
        raise ValueError(f"campaign contains failed steps: {failures}")


def validate_finish_completion(output, cir_results):
    output = Path(output)
    measurements_path = output / 'measurements.json'
    marker = json.loads((output / 'completed.json').read_text())
    measurements = json.loads(measurements_path.read_text())
    names = [record.get('name') for record in measurements]
    if any(not isinstance(name, str) for name in names) or len(set(names)) != len(names):
        raise ValueError('finish measurements contain missing or duplicate step names')
    expected = finish_expected_exit_codes(measurements, cir_results)
    if marker.get('schema') != 1 or marker.get('status') != 'complete':
        raise ValueError('finish completion marker has no successful schema/status')
    if marker.get('step_count') != len(measurements) or marker.get('steps') != names:
        raise ValueError('finish completion marker does not bind the measured steps')
    if marker.get('expected_exit_codes') != expected:
        raise ValueError('finish completion marker has an unexpected exit-code policy')
    if marker.get('measurements_sha256') != digest(measurements_path):
        raise ValueError('finish measurement digest differs from completion marker')
    failures = [record['name'] for record in measurements
                if record.get('exit_code') not in expected[record['name']]]
    if failures:
        raise ValueError(f'finish campaign contains failed steps: {failures}')


def instrumented(cache, runner, php_source, evaluation):
    whole = evaluation/'php-cli.bc'
    helper = runner.output/'instrument'
    for name in ['instrument', 'closure']:
        command = ['clang++-21', '-std=c++17', '-O1', '-I/usr/lib/llvm-21/include', HERE/(name+'.cpp'),
            '-L/usr/lib/llvm-21/lib', '-lLLVM-21', '-o', runner.output/name]
        require_success(runner.run(name+'-build', command, timeout=300), name+'-build')
    modified, sites = runner.output/'php-instrumented.bc', runner.output/'sites.json'
    require_success(runner.run('instrument-whole', [helper, whole, modified, sites], timeout=600), 'instrument-whole')
    require_success(runner.run('instrument-verify', ['opt-21', '-passes=verify', '-disable-output', modified], timeout=600), 'instrument-verify')
    manifest = json.loads((evaluation/'link-manifest.json').read_text())
    php = php_source
    asm = [php/x['object'] for x in manifest['objects'] if x.get('reason') == 'native-assembly-boundary']
    binary = runner.output/'php-instrumented'
    require_success(runner.run('instrument-link', ['clang-21', '-O0', '-no-pie', '-Wl,--export-dynamic', modified,
        HERE/'target_runtime.c', *asm, '-lm', '-o', binary], timeout=600), 'instrument-link')
    require_success(runner.run('instrumented-runtime', [binary, '-n', HERE/'runtime.php'], timeout=180), 'instrumented-runtime')
    require_success(runner.run('instrument-symbols', ['nm', '-n', binary]), 'instrument-symbols')
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
    require_success(runner.run('native-closure-audit', [runner.output/'closure', whole, roots], timeout=600), 'native-closure-audit')


def gcc_profile(cache, runner):
    """GCC dumps for the path-free PHP tree: the tree is built by build_php_pathfree.sh with gcc.

    Each unit is compiled from the spelling make used, with the tree as the working
    directory, so the dumps name sources relative to the tree.
    """
    php = cache / 'root/php-gcc/php-src'
    environment = dict(os.environ, PHP_CC='gcc', PHP_CXX='g++')
    environment['LD_LIBRARY_PATH'] = str(cache / 'root/deps/usr/lib/x86_64-linux-gnu')
    require_success(runner.run('gcc-tree', [HERE/'build_php_pathfree.sh', cache/'root/php-gcc', cache/'root/php-src',
        cache/'root/deps/usr'], env=environment, timeout=3600), 'gcc-tree')
    require_success(runner.run('gcc-runtime', [php/'sapi/cli/php', '-n', HERE/'runtime.php'], env=environment), 'gcc-runtime')
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
            *flags,'-c',recipe['source_arg'],'-o',output],cwd=php,timeout=600)
        require_success(success, 'gcc-'+str(i)+'-gimple')
        manifest['objects'].append(dict(object=target,source=str(source),source_sha256=digest(source),
            imported=success,command=recipe['command']))
        (runner.output/'gcc-manifest.json').write_text(json.dumps(recorded(manifest),indent=2)+'\n')


def cir_profile(cache, runner, php_source):
    build=cache/'cir-full-build'
    if not runner.run('cir-build', ['cmake','--build',build,'--target','clang','--parallel','8'],timeout=14400):
        raise RuntimeError('CIR compiler build failed; source-level CIR rejections cannot be classified')
    compiler=build/'bin/clang'
    # Each entry: name, source as the command names it, working directory, flags.
    fixture_inputs=[('dispatch','dispatch.c',HERE,[]),('platform','platform.c',HERE,[]),
        ('cleanup','cleanup.cpp',HERE,['-std=c++17']),
        ('cleanup-windows','cleanup.cpp',HERE,['-std=c++17','--target=x86_64-pc-windows-msvc','-fexceptions','-fcxx-exceptions'])]
    php=php_source
    commands, linked, _=recipes(php)
    for target in linked:
        recipe=commands[target]
        if Path(recipe['source']).suffix.lower() in ['.s','.asm']:continue
        flags=recipe['command'][1:recipe['command'].index('-c')]
        fixture_inputs.append(('php-'+target.replace('/','-'),recipe['source_arg'],php,flags))
    results=[]
    for name, source, directory, flags in fixture_inputs:
        result=runner.run(name+'-cir', [compiler,'-fclangir','-S','-Xclang','-emit-cir',*flags,
            source,'-o',runner.output/(name+'.cir')],cwd=directory,timeout=600)
        measurement=runner.measurements[-1]
        if not result and (measurement.get('stopped') is not None
                           or measurement.get('exit_code') != 1):
            raise RuntimeError(f"{name}-cir failed outside the classified unsupported-source outcome")
        cir_path = runner.output/(name+'.cir')
        results.append(dict(
            name=name,
            imported=result,
            exit_code=measurement['exit_code'],
            classification=("emitted" if result else "frontend rejected this translation unit; retained as unsupported evidence"),
            stderr_sha256=digest(runner.output/(name+'-cir.err')),
            **(dict(output_path=str(cir_path.resolve()), output_sha256=digest(cir_path)) if result else {}),
        ))
        (runner.output/'cir-imports.json').write_text(json.dumps(recorded(results),indent=2)+'\n')


def main():
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--cache',type=Path,required=True)
    parser.add_argument('--stage',choices=['all','gcc','cir'],default='all')
    parser.add_argument('--php-binary',type=Path)
    parser.add_argument('--php-source',type=Path)
    parser.add_argument('--stock-dvf',type=Path)
    parser.add_argument('--campaign',type=Path,help='the campaign output directory (default: CACHE/campaign)')
    parser.add_argument('--output',type=Path,help='the finish output directory (default: CACHE/finish)')
    parser.add_argument('--evaluation',type=Path,help='the linked-module evaluation directory (php-cli.bc, link-manifest.json)')
    args=parser.parse_args();cache=args.cache.resolve()
    campaign=args.campaign.resolve() if args.campaign else cache/'campaign'
    ready=campaign/'completed.json'
    while not ready.exists():time.sleep(1)
    validate_campaign_completion(cache, campaign)
    runner=EnforcedRunner(args.output.resolve() if args.output else cache/('finish' if args.stage=='all' else 'finish-'+args.stage))
    if args.stage=='gcc':
        gcc_profile(cache,runner)
        return
    if args.stage=='cir':
        if args.php_source is None:
            parser.error('--php-source is required for --stage cir')
        cir_profile(cache,runner,args.php_source.resolve())
        return
    if args.php_binary is None or args.php_source is None or args.stock_dvf is None:
        parser.error('--php-binary, --php-source, and --stock-dvf are required for --stage all')
    php_binary = args.php_binary.resolve()
    php_source = args.php_source.resolve()
    stock_dvf = args.stock_dvf.resolve()
    if not php_binary.is_file() or not stock_dvf.is_file() or not (php_source/'Zend/zend_API.c').is_file():
        parser.error('the supplied PHP source tree and measured binaries must exist')
    require_success(runner.run('binary-retry', [cache/'python/bin/python', HERE/'binary_cases.py', '--fixture', campaign/'layout-clang-21',
        '--php', php_binary, '--output', runner.output/'binary-results.json'], timeout=300), 'binary-retry')
    for candidate, binary in [('stock',stock_dvf),
                              ('compact',cache/'compact-svf/svf-build/bin/dvf')]:
        for name, flags in [('constants',['-model-consts=true']),
                            ('arrays',['-model-arrays=true']),
                            ('constants-arrays',['-model-consts=true','-model-arrays=true'])]:
            step='layout-'+candidate+'-'+name
            require_success(runner.run(step, svf_command(binary,'-dfs','-query=funptr','-stat','-print-query-pts',
                *flags,campaign/'layout_cases-promoted.bc')), step)
    instrumented(cache,runner,php_source,args.evaluation.resolve() if args.evaluation else cache/'whole-clang')
    gcc_profile(cache,runner)
    cir_profile(cache,runner,php_source)
    measurements_path=runner.output/'measurements.json'
    measurements=json.loads(measurements_path.read_text())
    names=[record['name'] for record in measurements]
    if len(set(names)) != len(names):
        raise RuntimeError('finish campaign measurements contain duplicate step names')
    cir_results=json.loads((runner.output/'cir-imports.json').read_text())
    expected_codes=finish_expected_exit_codes(measurements, cir_results)
    failures=[record['name'] for record in measurements
              if record.get('exit_code') not in expected_codes[record['name']]]
    if failures:
        raise RuntimeError(f"finish campaign has failed steps: {failures}")
    (runner.output/'completed.json').write_text(json.dumps({
        'schema': 1, 'status': 'complete', 'step_count': len(measurements),
        'steps': names,
        'expected_exit_codes': expected_codes,
        'measurements_sha256': digest(measurements_path)}, indent=2)+'\n')
    validate_finish_completion(runner.output, cir_results)


if __name__=='__main__':main()
