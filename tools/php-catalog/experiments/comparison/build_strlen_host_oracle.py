#!/usr/bin/env python3
"""Run the host's strlen transfers against the locked PHP 8.5.4 CLI and record what PHP does.

    build_strlen_host_oracle.py --php PATH/TO/sapi/cli/php --work DIR --output OUT.json

The binary must be the locked CLI: its digest is checked against `php_cli_sha256` in
results/lock.json before anything runs. Each case is a PHP program run in its own process.
Display and log output are disabled, so the recorded stdout and exit status carry no paths.
"""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path


HERE = Path(__file__).resolve().parent
LOCK = json.loads((HERE / 'results' / 'lock.json').read_text())
PRELUDE = {'weak': '<?php\n', 'strict': '<?php\ndeclare(strict_types=1);\n'}

MATRIX_CLASSES = '''class S { public function __toString(): string { return "ab"; } }
class P {}
final class Q {}
'''

MATRIX_VALUES = {
    'int': '5',
    'float': '1.5',
    'bool': 'true',
    'null': 'null',
    'string': '"abc"',
    'array': '[1]',
    'stringable': 'new S',
    'plain_object': 'new P',
    'final_object': 'new Q',
}

MATRIX_BODY = '''$GLOBALS['dep'] = [];
set_error_handler(static function (int $no, string $msg): bool { $GLOBALS['dep'][] = $msg; return true; });
$v = %(value)s;
try {
    $out = ['outcome' => 'return', 'value' => strlen($v)];
} catch (\\Throwable $e) {
    $out = ['outcome' => 'throw', 'class' => get_class($e)];
}
$out['deprecations'] = $GLOBALS['dep'];
echo 'RESULT:', json_encode($out), "\\n";
'''

DESTRUCTOR = '''class D {
    public function __toString(): string { return "x"; }
    public function __destruct() { echo "destruct\\n"; throw new RuntimeException("d"); }
}
'''

BEHAVIOURS = {
    'tostring_normal': ('weak', '''class S { public function __toString(): string { return "ab"; } }
$n = strlen(new S);
echo 'RESULT:', json_encode(['value' => $n]), "\\n";
'''),
    'tostring_throws': ('weak', '''class S { public function __toString(): string { throw new RuntimeException("t"); } }
try {
    $out = ['outcome' => 'return', 'value' => strlen(new S)];
} catch (RuntimeException $e) {
    $out = ['outcome' => 'throw', 'class' => get_class($e)];
}
echo 'RESULT:', json_encode($out), "\\n";
'''),
    'tostring_returns_int': ('weak', '''class S { public function __toString() { return 42; } }
try {
    $out = ['outcome' => 'return', 'value' => strlen(new S)];
} catch (\\Throwable $e) {
    $out = ['outcome' => 'throw', 'class' => get_class($e)];
}
echo 'RESULT:', json_encode($out), "\\n";
'''),
    'tostring_fatal': ('weak', '''class S { public function __toString(): string { trigger_error("fatal", E_USER_ERROR); return "x"; } }
echo "before\\n";
$n = strlen(new S);
echo "after\\n";
'''),
    'tostring_suspends': ('weak', '''class S { public function __toString(): string { Fiber::suspend("s"); return "ab"; } }
$f = new Fiber(function () {
    $n = strlen(new S);
    echo 'RESULT:', json_encode(['value' => $n]), "\\n";
});
$s = $f->start();
echo "suspended:", $s, "\\n";
$f->resume();
'''),
    'tostring_exits': ('weak', '''class S { public function __toString(): string { exit(3); } }
echo "before\\n";
$n = strlen(new S);
echo "after\\n";
'''),
    'destructor_on_new_temporary': ('weak', DESTRUCTOR + '''try {
    $out = ['outcome' => 'return', 'value' => strlen(new D)];
} catch (RuntimeException $e) {
    $out = ['outcome' => 'throw', 'class' => get_class($e), 'message' => $e->getMessage()];
}
echo 'RESULT:', json_encode($out), "\\n";
'''),
    'destructor_on_call_result': ('weak', DESTRUCTOR + '''function mk(): D { return new D; }
try {
    $out = ['outcome' => 'return', 'value' => strlen(mk())];
} catch (RuntimeException $e) {
    $out = ['outcome' => 'throw', 'class' => get_class($e), 'message' => $e->getMessage()];
}
echo 'RESULT:', json_encode($out), "\\n";
'''),
    'destructor_on_held_variable': ('weak', '''class D {
    public function __toString(): string { return "x"; }
    public function __destruct() { echo "destruct\\n"; }
}
$o = new D;
$n = strlen($o);
echo 'RESULT:', json_encode(['value' => $n]), "\\n";
'''),
    'destructor_after_unset_in_tostring': ('weak', '''class D {
    public function __toString(): string { unset($GLOBALS['o']); return "x"; }
    public function __destruct() { echo "destruct\\n"; throw new RuntimeException("d"); }
}
$o = new D;
try {
    $out = ['outcome' => 'return', 'value' => strlen($o)];
} catch (RuntimeException $e) {
    $out = ['outcome' => 'throw', 'class' => get_class($e), 'message' => $e->getMessage()];
}
echo 'RESULT:', json_encode($out), "\\n";
'''),
    'destructor_after_array_cleared_in_tostring': ('weak', '''class D {
    public function __toString(): string { $GLOBALS['arr'] = []; return "x"; }
    public function __destruct() { echo "destruct\\n"; throw new RuntimeException("d"); }
}
$arr = [new D];
try {
    $out = ['outcome' => 'return', 'value' => strlen($arr[0])];
} catch (RuntimeException $e) {
    $out = ['outcome' => 'throw', 'class' => get_class($e), 'message' => $e->getMessage()];
}
echo 'RESULT:', json_encode($out), "\\n";
'''),
    'diagnostic_handler_throws': ('weak', '''set_error_handler(static function (int $no, string $msg): bool { throw new ErrorException($msg); });
try {
    $out = ['outcome' => 'return', 'value' => strlen(null)];
} catch (ErrorException $e) {
    $out = ['outcome' => 'throw', 'class' => get_class($e)];
}
echo 'RESULT:', json_encode($out), "\\n";
'''),
    'constant_string_lengths': ('weak', '''echo 'RESULT:', json_encode(['multibyte' => strlen("\\u{e9}"), 'nul' => strlen("a\\0b"), 'empty' => strlen("")]), "\\n";
'''),
}


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run_case(php, work, case_id, source):
    path = work / f'{case_id}.php'
    path.write_text(source)
    completed = subprocess.run(
        [str(php), '-n', '-d', 'display_errors=0', '-d', 'log_errors=0', path.name],
        cwd=work, capture_output=True)
    stdout = completed.stdout.decode('utf-8', errors='replace')
    result = None
    for line in stdout.splitlines():
        if line.startswith('RESULT:'):
            result = json.loads(line[len('RESULT:'):])
    return {'exit_status': completed.returncode, 'stdout': stdout, 'result': result}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--php', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args(argv)
    digest = sha256(args.php)
    if digest != LOCK['php_cli_sha256']:
        raise SystemExit(f'{args.php} is not the locked CLI: {digest}')
    args.work.mkdir(parents=True, exist_ok=True)

    cases = []
    for mode in ('weak', 'strict'):
        for name, value in MATRIX_VALUES.items():
            source = PRELUDE[mode] + MATRIX_CLASSES + MATRIX_BODY % {'value': value}
            cases.append({'id': f'parser-{mode}-{name}', 'mode': mode, 'transfer': 'parser-predicate',
                          'input': name, 'source': source})
    for name, (mode, body) in BEHAVIOURS.items():
        cases.append({'id': name, 'mode': mode, 'transfer': name, 'input': None,
                      'source': PRELUDE[mode] + body})
    for case in cases:
        case.update(run_case(args.php, args.work, case['id'], case.pop('source')))
    document = {'php_cli_sha256': digest, 'php_source_commit': LOCK['php_commit'], 'cases': cases}
    args.output.write_text(json.dumps(document, indent=2) + '\n')
    print(f'{len(cases)} cases written to {args.output}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
