#!/usr/bin/env python3
"""Lower every successfully emitted PHP CIR unit to LLVM IR and verify it."""
import argparse
import hashlib
import json
from pathlib import Path
import sys

from evaluation import EnforcedRunner


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--campaign', type=Path, required=True,
                        help='cir-campaign.json from the CIR emission campaign')
    parser.add_argument('--output', type=Path, required=True,
                        help='directory for lowered LLVM IR and command evidence')
    parser.add_argument('--summary', type=Path, required=True,
                        help='summary JSON output path')
    parser.add_argument('--opt', default='opt-21')
    args = parser.parse_args()

    campaign = json.loads(args.campaign.read_text())
    units = [x for x in campaign['units']
             if x['kind'] == 'php-c-object' and x['emitted']]
    runner = EnforcedRunner(args.output)
    results = []
    for unit in units:
        name = unit['name']
        command = list(unit['command'])
        cir_flag = command.index('-Xclang')
        if command[cir_flag + 1] != '-emit-cir':
            raise ValueError(f'Unexpected CIR emission command for {name}')
        del command[cir_flag:cir_flag + 2]
        command.insert(cir_flag, '-emit-llvm')
        output = args.output / (name + '.ll')
        command[command.index('-o') + 1] = str(output)

        lowered = runner.run(name + '-lower-llvm', command,
                             cwd=unit['working_directory'], timeout=600)
        verified = False
        if lowered and output.is_file() and output.stat().st_size:
            verified = runner.run(name + '-verify-llvm',
                                  [args.opt, '-passes=verify', '-disable-output', output],
                                  cwd=unit['working_directory'], timeout=600)
        results.append({
            'name': name,
            'object': unit['object'],
            'source': unit['source'],
            'source_sha256': unit['source_sha256'],
            'cir_sha256': unit['output']['sha256'],
            'lowered': lowered,
            'llvm_verified': bool(verified),
            'llvm_bytes': output.stat().st_size if output.is_file() else None,
            'llvm_sha256': sha256(output) if output.is_file() and output.stat().st_size else None,
            'lower_command': command,
            'verify_command': [args.opt, '-passes=verify', '-disable-output', str(output)],
        })
        args.summary.parent.mkdir(parents=True, exist_ok=True)
        args.summary.write_text(json.dumps({
            'compiler_source_revision': campaign['compiler_source_revision'],
            'php_source_commit': campaign['php_source_commit'],
            'profile': campaign['profile'],
            'method': 'Reinvoke CIR-enabled Clang with the original profile flags, replacing CIR text emission with LLVM IR emission; verify each LLVM module with opt-21.',
            'cgroup_limit_bytes': 10737418240,
            'swap_limit_bytes': 0,
            'input_count': len(units),
            'lowered_count': sum(x['lowered'] for x in results),
            'verified_count': sum(x['llvm_verified'] for x in results),
            'output_directory': str(args.output.resolve()),
            'units': results,
        }, indent=2) + '\n')

    return 0 if all(x['lowered'] and x['llvm_verified'] for x in results) else 1


if __name__ == '__main__':
    sys.exit(main())
