#!/usr/bin/env python3
"""Run the recorded PHP 8.5 runtime fixtures on one PHP and compare stdout.

The recorded outputs come from the locked PHP 8.5.4 witness runs. Each
fixture's stdout must equal every recorded output it is paired with, byte for
byte, so a PHP 8.5.x release other than 8.5.4 is checked against 8.5.4's
recorded behaviour. Exactly one of --php (a PHP executable) or --docker (an
image whose entry point is php) must be given. Exits non-zero on the first
mismatch.
"""

import argparse
import subprocess
import sys
from pathlib import Path


HERE = Path(__file__).resolve().parent
RUNTIME = HERE / 'results/runtime'

# Fixture -> recorded stdout files. Each pair is the TAILCALL and CALL build of
# the same locked PHP source, which must agree with each other and with every
# other platform's build.
PAIRS = {
    'runtime-strlen-weak-scalars.php': ['php-tailcall-strlen-weak-scalars.out', 'php-call-strlen-weak-scalars.out'],
    'runtime-strlen-strict-scalars.php': ['php-tailcall-strlen-strict-scalars.out', 'php-call-strlen-strict-scalars.out'],
    'runtime-strlen-release.php': ['php-tailcall-strlen-release.out', 'php-call-strlen-release.out'],
    'runtime-strlen-abrupt-outcomes.php': ['php-tailcall-strlen-abrupt-outcomes.out', 'php-call-strlen-abrupt-outcomes.out'],
    'runtime-strlen-parser-outcomes-weak.php': ['php-tailcall-strlen-parser-outcomes-weak.out', 'php-call-strlen-parser-outcomes-weak.out'],
    'runtime-strlen-parser-outcomes-strict.php': ['php-tailcall-strlen-parser-outcomes-strict.out', 'php-call-strlen-parser-outcomes-strict.out'],
    'runtime.php': ['php-tailcall-reference.out', 'php-call-reference.out'],
    'runtime-strict.php': ['php-tailcall-strict-reference.out', 'php-call-strict-reference.out'],
}


def php_command(args, fixture):
    if args.docker:
        return ['docker', 'run', '--rm', '-v', f'{HERE}:/w', '-w', '/w', args.docker, 'php', '-n', fixture]
    return [args.php, '-n', fixture]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--php', help='path to a php executable')
    source.add_argument('--docker', help='docker image whose entry point runs php')
    args = parser.parse_args()

    failures = []
    for fixture, outputs in PAIRS.items():
        result = subprocess.run(php_command(args, fixture), cwd=HERE, capture_output=True)
        if result.returncode != 0:
            failures.append(f'{fixture}: exit {result.returncode}')
            continue
        for name in outputs:
            expected = (RUNTIME / name).read_bytes()
            if result.stdout != expected:
                failures.append(f'{fixture}: stdout differs from {name}')
            else:
                print(f'ok {fixture} == {name}')
    if failures:
        print('\n'.join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
