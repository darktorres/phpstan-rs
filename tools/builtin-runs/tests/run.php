<?php
// The recorder's own tests: the table and what is kept beside it, the cache
// key, the selection and the stub index. Run with `php tests/run.php`; exits 1
// when a check fails.

declare(strict_types=0);

error_reporting(E_ALL);
ini_set('display_errors', '1');

require_once dirname(__DIR__) . '/lib/seen.php';
require_once dirname(__DIR__) . '/lib/probes.php';
require_once dirname(__DIR__) . '/lib/child.php';
require_once dirname(__DIR__) . '/lib/plan.php';
require_once dirname(__DIR__) . '/lib/unit.php';
require_once dirname(__DIR__) . '/lib/protocol.php';
require_once dirname(__DIR__) . '/lib/workdir.php';
require_once dirname(__DIR__) . '/lib/pool.php';
require_once dirname(__DIR__) . '/lib/stats.php';
require_once dirname(__DIR__) . '/lib/select.php';
require_once dirname(__DIR__) . '/lib/table.php';
require_once dirname(__DIR__) . '/lib/cache.php';
require_once dirname(__DIR__) . '/lib/sidecar.php';
require_once dirname(__DIR__) . '/lib/coverage.php';
require_once dirname(__DIR__) . '/lib/sandbox.php';
require_once dirname(__DIR__) . '/lib/limits.php';
require_once dirname(__DIR__) . '/lib/canary.php';
require_once dirname(__DIR__) . '/lib/groups.php';
require_once __DIR__ . '/fixtures.php';

$failures = [];
$checks = 0;

/** Records a check. */
function check(string $name, bool $holds, string $detail = ''): void
{
    global $failures, $checks;
    $checks++;
    if (!$holds) {
        $failures[] = $name . ($detail === '' ? '' : ": $detail");
    }
}

$scratch = work_root() . '/builtin-runs-tests-' . getmypid();
@mkdir($scratch, 0777, true);

// The table: sorted as bytes, one row to a line, with the trailing newline.
check('table_text sorts as bytes', table_text(["b\t\tprobed\t", "a\t\tprobed\t", "A\t\tprobed\t"]) === "A\t\tprobed\t\na\t\tprobed\t\nb\t\tprobed\t\n");
check('table_text of no rows is one newline', table_text([]) === "\n");

// Merging a partial run: the selected builtins' rows go, the others stay.
$selected = static fn (string $builtin): bool => $builtin === 'ds\\deque::push';
$existing = ["ds\\deque::push\t\tprobed\t", "ds\\deque::push\t0\tcalls\t", "ds\\map::put\t\tprobed\t", "decimal\\decimal::__tostring\t\tprobed\t"];
$merged = merge_rows($existing, ["ds\\deque::push\t\tprobed\t"], $selected);
sort($merged, SORT_STRING);
check('merge removes the selected builtin and keeps the rest', $merged === ["decimal\\decimal::__tostring\t\tprobed\t", "ds\\deque::push\t\tprobed\t", "ds\\map::put\t\tprobed\t"], (string) json_encode($merged));
check('merge keeps a row of a builtin named in a by: place', merge_rows(["x::y\tby:z:0\tcalls\t"], [], static fn (string $b): bool => $b === 'x::z') === ["x::y\tby:z:0\tcalls\t"]);

// A table written atomically: the content is there, and no temporary file is left.
$path = "$scratch/table.tsv";
write_atomic($path, "one\n");
check('write_atomic writes the content', file_get_contents($path) === "one\n");
write_atomic($path, "two\n");
check('write_atomic replaces the old file', file_get_contents($path) === "two\n");
check('write_atomic leaves no temporary file', count(glob("$scratch/*.tmp.*") ?: []) === 0);
// A write that cannot complete leaves the old file.
$child = escapeshellarg(PHP_BINARY) . ' -r ' . escapeshellarg(
    'require ' . var_export(dirname(__DIR__) . '/lib/table.php', true) . '; write_atomic(' . var_export("$scratch/no-such-dir/x.tsv", true) . ', "x");'
);
exec($child . ' 2>/dev/null', $output, $status);
check('write_atomic exits 1 when the directory is missing', $status === 1, "status $status");
check('the failed write left the old file', file_get_contents($path) === "two\n");

// The committed table is in the grammar build.rs reads: four tab-separated
// fields, the builtin and the detail lowercased, the place and the kind known.
$committed = dirname(__DIR__, 3) . '/crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv';
$bad = [];
$kinds = [];
foreach (file($committed, FILE_IGNORE_NEW_LINES) as $line) {
    $fields = explode("\t", $line);
    if (count($fields) !== 4) {
        $bad[] = "not four fields: $line";
        continue;
    }
    [$builtin, $place, $kind, $detail] = $fields;
    $kinds[$kind] = true;
    if ($builtin === '' || $builtin !== strtolower($builtin) || $detail !== strtolower($detail)) {
        $bad[] = "not lowercased: $line";
    }
    if (!preg_match('/^(new:|by:[a-z0-9_]+:)?\d+(\[\])?$|^$/', $place)) {
        $bad[] = "place: $line";
    }
    if (!in_array($kind, ['probed', 'unnamed', 'calls', 'method', 'wrapper', 'releases'], true)) {
        $bad[] = "kind: $line";
    }
}
check('committed table rows follow the grammar', $bad === [], implode('; ', array_slice($bad, 0, 3)));
check('committed table has the five kinds', count($kinds) === 5, implode(',', array_keys($kinds)));

// Counts of rows by kind, and of the `by:` rows.
$counts = row_counts(["a\t\tprobed\t", "a\tby:m:0\tcalls\t", "a\t0\tcalls\t"]);
check('row_counts counts kinds and by: rows', $counts === ['by' => 1, 'calls' => 2, 'probed' => 1], (string) json_encode($counts));

// Two runs of a unit: `probed` and `releases` only where both agree.
[$rows, $differs] = reconcile(
    ["a\t\tprobed\t" => true, "a\t0\tcalls\t" => true, "a\t0\treleases\t" => true],
    ["a\t\tprobed\t" => true, "a\t1\tcalls\t" => true],
);
sort($rows, SORT_STRING);
check('reconcile keeps probed when both found it', in_array("a\t\tprobed\t", $rows, true));
[$alone] = reconcile(["a\t\tprobed\t" => true], []);
check('reconcile drops probed when one run found it', $alone === []);
check('reconcile keeps calls seen in either run', in_array("a\t0\tcalls\t", $rows, true) && in_array("a\t1\tcalls\t", $rows, true));
check('reconcile drops releases seen in one run', !in_array("a\t0\treleases\t", $rows, true));
check('reconcile reports that the runs differed', $differs);
[, $same] = reconcile(["a\t\tprobed\t" => true], ["a\t\tprobed\t" => true]);
check('reconcile reports equal runs as equal', !$same);

// The cache key changes with each thing that can change a result, and not with the order of the unit's parts.
$unit = ['method', 'Ds\\Deque', 'push', 0, 0];
$base = ['recorder' => 'r', 'binary' => '1/2', 'manifest' => 'm', 'unit' => $unit, 'signature' => 's', 'skip' => 'off', 'sandbox' => 'none', 'known' => null];
$key = cache_key($base);
check('cache key is stable', $key === cache_key($base));
foreach (['recorder' => 'r2', 'binary' => '1/3', 'manifest' => 'm2', 'signature' => 's2', 'skip' => 'dead', 'sandbox' => 'bwrap', 'known' => ['ds\\deque::push' => 1]] as $field => $value) {
    check("cache key changes with $field", cache_key([$field => $value] + $base) !== $key);
}
check('cache key changes with the unit', cache_key(['unit' => ['method', 'Ds\\Deque', 'pop', 0, 0]] + $base) !== $key);

// The manifest: Core and FFI have no info hash (their text differs between runs),
// the other extensions do, and each lists what it provides.
$manifest = manifest();
check('Core has no info hash', array_key_exists('Core', $manifest['extensions']) && $manifest['extensions']['Core']['info'] === null);
check('a stable extension has an info hash', is_string($manifest['extensions']['standard']['info'] ?? null));
check('an extension lists what it provides', in_array('strlen', $manifest['extensions']['Core']['provides'], true) || in_array('strlen', $manifest['extensions']['standard']['provides'], true));
check('manifest is equal on a second call', manifest() === $manifest);

// The selection: by regex, by extension, or both.
$byRegex = selector('#^decimal\\\\#', null);
check('a regex selects by lowercased name', $byRegex('decimal\\decimal::__tostring') && !$byRegex('ds\\deque::push'));
// `standard` is compiled into every PHP, so these hold whichever extensions the build loads.
$byExtension = selector(null, ['standard']);
check('an extension selects its functions', $byExtension('str_repeat') && !$byExtension('ds\\deque::push'));
$both = selector('#^str_#', ['standard']);
check('a regex and an extension select the intersection', $both('str_repeat') && !$both('str_no_such_function') && !$both('array_map'));
check('no selection selects everything', selector(null, null)('anything::at_all'));

// The stub index: its declarations are read, with the extension they belong to.
$stubs = stub_declarations();
check('the stub index has functions and classes', count($stubs) > 1000 && in_array('class', array_column($stubs, 0), true));

// The sidecar: the table's hash, and each run that wrote it.
$sidecar = json_decode(sidecar_text(["a\t\tprobed\t"], $manifest, 'rec', 'off', 'bwrap', [], ['selection' => [], 'flags' => [], 'totals' => []], []), true);
check('the sidecar hashes the table it describes', $sidecar['table']['sha256'] === hash('sha256', "a\t\tprobed\t\n"));
check('the sidecar records one run', count($sidecar['runs']) === 1);
check('the sidecar path drops .tsv', sidecar_path('/x/builtin-runs-8.5.tsv') === '/x/builtin-runs-8.5.meta.json');

// The pinned rows: every `by:` row of the committed table, and a sample of the
// others, must be in it. The recorder's equality with the old script is what
// the families show; this keeps those rows from being dropped by an edit.
$pinned = dirname(__DIR__, 3) . '/crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv';
$committedRows = array_flip(file($pinned, FILE_IGNORE_NEW_LINES) ?: []);
$missing = [];
foreach ((file(__DIR__ . '/known.tsv', FILE_IGNORE_NEW_LINES) ?: []) as $row) {
    if (!isset($committedRows[$row])) {
        $missing[] = $row;
    }
}
check('the pinned rows are in the committed table', $missing === [], implode('; ', array_slice($missing, 0, 3)));

// The fixtures: units of user classes, run as the recorder runs them. They need
// FFI for the limits and the stream of the children; without it they are skipped.
if (!class_exists('FFI') || ini_get('ffi.enable') !== '1') {
    echo "skipped the fixtures: run with -d ffi.enable=1\n";
} else {
    Limits::init();
    Workdir::create();
    $pidFile = work_root() . '/builtin-runs-stubborn-' . getmypid();
    define('FIXTURE_PIDFILE', $pidFile);
    $all = static fn (string $builtin): bool => str_starts_with($builtin, 'fixture');
    $rowsOf = static fn (array $lines): array => array_keys($lines);

    // A held unit: the store keeps the callable, and the call runs it.
    $stored = $rowsOf(run_unit(['held', 'FixtureStore', 'store', 0, 0], $all, 'off', []));
    check('a kept callable is called from the other method', in_array("fixturestore::call\tby:store:0\tcalls\t", $stored, true), (string) json_encode($stored));
    check('the method it is called from is recorded as reached', in_array("fixturestore::call\t\tprobed\t", $stored, true));

    // The released rule: a method that drops the callable is not run, when the
    // builtin already has a row (the phase-A rows), and is run when it has none.
    $without = $rowsOf(run_unit(['held', 'FixtureStore', 'drop', 0, 0], $all, 'released', []));
    check('a dropped callable runs the method with no phase-A row', in_array("fixturestore::drop\t\tprobed\t", $without, true), (string) json_encode($without));
    $with = $rowsOf(run_unit(['held', 'FixtureStore', 'drop', 0, 0], $all, 'released', ['fixturestore::drop' => 1]));
    check('a released probe skips a method with a phase-A row', !in_array("fixturestore::drop\t\tprobed\t", $with, true), (string) json_encode($with));

    // A throwing constructor: no object, so no child of the unit runs.
    $before = Stats::$children;
    $broken = run_unit(['held', 'FixtureBroken', 'store', 0, 0], $all, 'dead', []);
    check('a constructor that throws records nothing', $broken === [], (string) json_encode(array_keys($broken)));

    // A method that never returns: one timeout, from the survey, and no rows.
    $timeouts = Stats::$timeouts;
    $hung = run_unit(['held', 'FixtureHang', 'hang', 0, 0], $all, 'dead', []);
    check('a hanging method records nothing and costs one timeout', $hung === [] && Stats::$timeouts === $timeouts + 1, (string) json_encode(array_keys($hung)) . ' timeouts ' . (Stats::$timeouts - $timeouts));

    // A method that exits the process: nothing comes back.
    $exited = run_unit(['method', 'FixtureExit', 'exits', 0, 0], $all, 'off', []);
    check('a method that exits records nothing', $exited === [], (string) json_encode(array_keys($exited)));

    // A process that ignores the alarm is killed, with the process it left behind.
    @unlink($pidFile);
    run_unit(['method', 'FixtureStubborn', 'stubborn', 0, 0], $all, 'off', []);
    $leftover = is_file($pidFile) ? (int) trim((string) file_get_contents($pidFile)) : 0;
    $alive = true;
    for ($i = 0; $i < 40 && $alive && $leftover > 0; $i++) {
        $alive = posix_kill($leftover, 0) && !str_contains((string) @file_get_contents("/proc/$leftover/stat"), ') Z');
        if ($alive) {
            usleep(100000);
        }
    }
    check('a process that ignores the alarm leaves no process behind', $leftover > 0 && !$alive, "pid $leftover");
    @unlink($pidFile);

    Workdir::remove();
}

// The integrity and drift checks of the committed table, against its sidecar:
// the table is the one the sidecar hashes; every probed builtin belongs to an
// extension that a recorded run loaded; and every stub declaration of a recorded
// extension has a row, or is a gap the sidecar records.
$tablePath = dirname(__DIR__, 3) . '/crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv';
$sidecarPath = sidecar_path($tablePath);
if (!is_file($sidecarPath)) {
    echo "skipped the integrity and drift checks: no sidecar beside the table\n";
} else {
    $recorded = json_decode((string) file_get_contents($sidecarPath), true);
    $tableRows = read_rows($tablePath);
    check('the sidecar hashes the committed table', ($recorded['table']['sha256'] ?? '') === hash_file('sha256', $tablePath));
    $provided = [];
    $loaded = [];
    foreach ($recorded['runs'] as $run) {
        foreach ($run['extensions'] as $extension => $info) {
            $loaded[strtolower((string) $extension)] = true;
            foreach ($info['provides'] as $name) {
                $provided[strtolower($name)] = true;
            }
        }
    }
    $probed = [];
    foreach ($tableRows as $row) {
        [$builtin, , $kind] = explode("\t", $row);
        if ($kind === 'probed') {
            $probed[$builtin] = true;
        }
    }
    $unprovided = [];
    foreach (array_keys($probed) as $builtin) {
        if (!isset($provided[explode('::', $builtin, 2)[0]])) {
            $unprovided[] = $builtin;
        }
    }
    check('every probed builtin is provided by a recorded extension', $unprovided === [], implode(', ', array_slice($unprovided, 0, 5)));
    $gaps = array_flip($recorded['gaps'] ?? []);
    $classesWithRows = [];
    foreach (array_keys($probed) as $builtin) {
        if (str_contains($builtin, '::')) {
            $classesWithRows[explode('::', $builtin, 2)[0]] = true;
        }
    }
    $drift = [];
    foreach (stub_declarations() as [$kind, $key, $name, $extension]) {
        if (!isset($loaded[strtolower($extension)])) {
            continue;
        }
        $hasRow = $kind === 'function' ? isset($probed[$key]) : isset($classesWithRows[$key]);
        if (!$hasRow && !isset($gaps["$extension\t$name"])) {
            $drift[] = "$extension $name";
        }
    }
    check('every stub declaration of a recorded extension has a row or a recorded gap', $drift === [], implode(', ', array_slice($drift, 0, 5)));
}

// The sandbox: the command that starts a worker in each mode, and the scratch space of each.
$bwrap = worker_command('/usr/bin/php', false, ['mode' => 'bwrap', 'cap' => 268435456]);
check('bwrap starts the worker with no network and a capped scratch space', $bwrap[0] === 'bwrap' && in_array('--unshare-net', $bwrap, true) && in_array('268435456', $bwrap, true) && str_ends_with((string) end($bwrap), '/worker.php'));
check('bwrap never mounts or names the temporary directory',array_filter($bwrap, static fn (string $argument): bool => str_contains($argument, '/tmp')) === []);
check('bwrap makes the worker home its capped scratch tmpfs', in_array('/home/worker', $bwrap, true) && in_array('HOME', $bwrap, true) && in_array('--tmpfs', $bwrap, true));
check('bwrap sets the allocator off unless --zend-mm', in_array('USE_ZEND_ALLOC', $bwrap, true) && in_array('0', $bwrap, true));
check('bwrap with --zend-mm leaves the allocator alone', !in_array('USE_ZEND_ALLOC', worker_command('/usr/bin/php', true, ['mode' => 'bwrap', 'cap' => 1]), true));
// A worker started under bwrap, with no input, ends with status 0. When bwrap
// cannot start here (an AppArmor rule, no user namespaces), the case is skipped.
$started = proc_open(
    worker_command(PHP_BINARY, false, ['mode' => 'bwrap', 'cap' => 268435456]),
    [0 => ['file', '/dev/null', 'r'], 1 => ['file', '/dev/null', 'w'], 2 => ['file', '/dev/null', 'w']],
    $pipes,
);
$status = is_resource($started) ? proc_close($started) : -1;
if ($status === 0) {
    check('a worker starts under bwrap', true);
} else {
    echo "skipped the bwrap start: bwrap did not start a worker here (status $status)\n";
}
$plain = worker_command('/usr/bin/php', false, ['mode' => 'none', 'cap' => 0]);
check('none is the PHP itself', $plain[0] === '/usr/bin/php' && !in_array('bwrap', $plain, true));
check('sudo-unshare is chosen by name only', worker_command('/usr/bin/php', false, ['mode' => 'sudo-unshare', 'cap' => 0])[0] === 'sudo');
check('the scratch space of a worker is at most 256 MiB', sandbox_cap(8) <= 256 * 1024 ** 2 && sandbox_cap(1) <= 256 * 1024 ** 2);

// The worker's modules: a lean worker loads the base modules and the ones it
// records, its needs first, with the settings the ini files give; and the
// groups of a PHP cover each loaded extension once.
$configured = array_keys(php_configuration(PHP_BINARY)['modules']);
$lean = lean_arguments(PHP_BINARY, []);
check('a lean worker starts without the ini directory, with the php.ini', $lean[0] === '-n' && $lean[1] === '-c' && $lean[2] === php_configuration(PHP_BINARY)['ini']);
check('a lean worker loads the base modules and no other', in_array('extension=posix', $lean, true) && in_array('extension=tokenizer', $lean, true) && !in_array('extension=imagick', $lean, true));
check('the global settings reach every lean worker', in_array('zend.assertions=1', $lean, true));
if (in_array('pdo_sqlite', $configured, true)) {
    $sqlite = lean_arguments(PHP_BINARY, ['pdo_sqlite']);
    check('a module loads after the module it needs', array_search('extension=pdo', $sqlite, true) < array_search('extension=pdo_sqlite', $sqlite, true));
}
if (in_array('igbinary', $configured, true)) {
    check('a module setting reaches its worker', in_array('igbinary.compact_strings=On', lean_arguments(PHP_BINARY, ['igbinary']), true));
    check('a module setting stays off the workers of other modules', !in_array('igbinary.compact_strings=On', lean_arguments(PHP_BINARY, ['msgpack']), true));
}
$groups = recording_groups(get_loaded_extensions(), compiled_extensions(PHP_BINARY), $configured);
$covered = [];
foreach ($groups as $group) {
    foreach ($group['extensions'] as $extension) {
        $covered[] = $extension;
    }
}
check('the groups cover each loaded extension once', count($covered) === count(get_loaded_extensions()) && count(array_unique($covered)) === count($covered));
foreach ($configured as $module) {
    $loads = (string) shell_exec(implode(' ', array_map('escapeshellarg', [PHP_BINARY, ...lean_arguments(PHP_BINARY, [$module]), '-r', 'echo 1;'])) . ' 2>&1');
    check("the group of $module loads with its needs", !str_contains($loads, 'Unable to load'), $loads);
}
try {
    recording_groups(['Mystery'], [], []);
    check('a loaded extension neither built in nor configured is refused', false);
} catch (RuntimeException) {
    check('a loaded extension neither built in nor configured is refused', true);
}

@array_map('unlink', glob("$scratch/*") ?: []);
@rmdir($scratch);

if ($failures !== []) {
    foreach ($failures as $failure) {
        fwrite(STDERR, "FAIL $failure\n");
    }
    fwrite(STDERR, count($failures) . " of $checks checks failed\n");
    exit(1);
}
echo "$checks checks passed\n";
