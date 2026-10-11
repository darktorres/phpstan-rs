<?php
// Records the user code each builtin runs while it is called: for every
// function and every method of every class PHP defines, which callables
// passed to it it invokes, which methods it calls on an object passed to
// it, which stream-wrapper methods it calls through a URL or a stream
// passed to it, and, for a method, which code its object was given when it
// was made that it runs. What a call shows is what it ran with the values
// given; a builtin can run more with others. Prints one line for each,
// tab-separated:
//
//   <builtin> <where> <kind> <detail>
//
// <builtin> is a function's name, or `Class::method`, lowercased. <where>
// is the parameter's position (from 0), followed by `[]` when what ran was
// reached through the elements of an array passed there, and prefixed by
// `new:` for a parameter of the class's constructor, or by `by:<method>:`
// for a parameter of another method of the class, called on the object
// before this one. <kind> is `calls` (the callable
// was invoked), `method` (the method <detail> was called on the object),
// `wrapper` (the wrapper method <detail> ran), or `releases` (a function
// given a callable at a parameter declared to take one kept none of it,
// nor of what it gave back, once it returned; a position with no such line
// may keep it). Every builtin that was
// called is listed with the kind `probed`; a builtin missing from the
// output is one this PHP does not define, or one no call reached (an
// abstract class's method, a constructor that would not take the values).
//
// The calls run in worker processes, which this script starts with
// `proc_open` as fresh PHP processes (worker.php). Each call runs in a child
// of its worker, in an empty directory, with an alarm, so that a call that
// ends the process, changes its state or blocks affects no other. Run it with
// the PHP whose behaviour is wanted:
//
//   php tools/builtin-runs/record.php > crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv
//
// A regular expression as the first argument limits it to the builtins
// whose lowercased name it matches; `--ext=NAME,...` limits it to the
// functions and methods of those extensions (both together must match).
// `--jobs=N` sets the number of workers (one for each processor by default):
// each takes the next unit from a queue as it finishes its last, so the
// machine stays busy to the end. `--php=BIN` records the PHP BIN instead of
// this one, and the workers are its processes. `--zend-mm` leaves the workers
// on PHP's own allocator; by default they run with `USE_ZEND_ALLOC=0`, which
// gives the same output about twice as fast.
//
// `--skip=dead` first runs one child per held unit that makes the object and
// calls the method it is given the probe for; when that yields no object,
// none of the unit's children runs. `--skip=released` does the same and also
// skips a child whose probe the object released, for a method that already
// has a `probed` row: the held units run after the others.
//
// `--out=FILE` writes the table to FILE and its sidecar beside it (the table's
// name without `.tsv`, and `.meta.json`): the SHA-256 of the table, what it was
// recorded with, and each run that wrote it. `--update=FILE` does the same for
// the rows of the builtins the selection selects: those rows are replaced by
// the new ones, and the others are kept. Both write nothing when a unit was
// lost. `--cache=DIR` (default `~/.cache/builtin-runs`) keeps each unit's rows
// under a key of everything that can change them, and `--no-cache` ignores it.
// `--verify` runs every unit twice, without the cache, and keeps the rows both
// runs agree on for `probed` and `releases`, and the rows either found for the
// others; the units where the runs differ are listed on standard error.
// `--coverage=FILE` lists the builtins with no `probed` row, and the stub
// declarations of the engine that have none, each with its reason.
//
// A worker that dies has its unit run once more on another worker. A unit
// that dies twice is reported on standard error, its rows are missing from
// the output, and the exit status is 1.
//
// `--list` prints the units and the children a run would make, by kind and
// for the classes with the most, and stops. `--stats=FILE` writes a JSON
// line for every child (the unit's kind, the builtin, where, its wall time
// in milliseconds and `ok`, `timeout` or `died`) and a last line with the
// totals, the histogram of wall times, the slowest children, the timeouts by
// class and each worker's peak memory. A line on standard error every five
// seconds gives the share of the children done, the time and the failures.

declare(strict_types=0);

error_reporting(0);
ini_set('display_errors', '0');
ini_set('memory_limit', '512M');

if (!function_exists('pcntl_fork')) {
    fwrite(STDERR, "pcntl is needed\n");
    exit(1);
}

require_once __DIR__ . '/lib/seen.php';
require_once __DIR__ . '/lib/probes.php';
require_once __DIR__ . '/lib/child.php';
require_once __DIR__ . '/lib/plan.php';
require_once __DIR__ . '/lib/unit.php';
require_once __DIR__ . '/lib/protocol.php';
require_once __DIR__ . '/lib/workdir.php';
require_once __DIR__ . '/lib/sandbox.php';
require_once __DIR__ . '/lib/pool.php';
require_once __DIR__ . '/lib/stats.php';
require_once __DIR__ . '/lib/select.php';
require_once __DIR__ . '/lib/table.php';
require_once __DIR__ . '/lib/cache.php';
require_once __DIR__ . '/lib/sidecar.php';
require_once __DIR__ . '/lib/coverage.php';

$arguments = array_values(array_filter(array_slice($argv, 1), static fn (string $argument): bool => !str_starts_with($argument, '--')));
$only = isset($arguments[0]) ? (string) $arguments[0] : null;
$php = option($argv, 'php') ?? PHP_BINARY;
$zendMm = in_array('--zend-mm', $argv, true);
$sandboxMode = option($argv, 'sandbox') ?? 'bwrap';
if (!in_array($sandboxMode, ['bwrap', 'sudo-unshare', 'none'], true)) {
    fwrite(STDERR, "--sandbox is bwrap, sudo-unshare or none\n");
    exit(2);
}
$skip = option($argv, 'skip') ?? 'off';
if (!in_array($skip, ['off', 'dead', 'released'], true)) {
    fwrite(STDERR, "--skip is off, dead or released\n");
    exit(2);
}
$extensions = option($argv, 'ext') === null ? null : array_values(array_filter(explode(',', (string) option($argv, 'ext'))));
$out = option($argv, 'out');
$update = option($argv, 'update');
$coverage = option($argv, 'coverage');
if ($out !== null && $update !== null) {
    fwrite(STDERR, "--out and --update are one or the other\n");
    exit(2);
}
$verify = in_array('--verify', $argv, true);
$cacheDir = in_array('--no-cache', $argv, true) || $verify ? null : (option($argv, 'cache') ?? (getenv('HOME') ?: posix_getpwuid(posix_geteuid())['dir']) . '/.cache/builtin-runs');

sweep_stale_directories();
pcntl_async_signals(true);
$stop = static function (int $signal): void {
    Pool::abandon();
    exit(128 + $signal);
};
pcntl_signal(SIGINT, $stop);
pcntl_signal(SIGTERM, $stop);

$jobs = jobs($argv);
// A worker's children write at most 64 MiB each, four of them at once in the
// worst case of the measured limits, so scratch space smaller than 256 MiB is refused.
$sandbox = ['mode' => $sandboxMode, 'cap' => $sandboxMode === 'bwrap' ? sandbox_cap($jobs) : 0];
if ($sandboxMode === 'bwrap' && $sandbox['cap'] < 256 * 1024 ** 2) {
    fwrite(STDERR, 'only ' . intdiv($sandbox['cap'], 1024 ** 2) . " MiB of scratch space per worker: free memory or use fewer --jobs\n");
    exit(1);
}
$plan = plan_run($php, $only, $extensions, $zendMm, $sandbox);
if (in_array('--list', $argv, true)) {
    echo plan_summary($plan['units'], $plan['weights']);
    exit(0);
}
$wanted = selector($only, $extensions);
foreach ([$out, $update, $coverage, $cacheDir] as $path) {
    if ($path !== null) {
        require_free_space($path);
    }
}
$statsFile = option($argv, 'stats');
if ($statsFile !== null) {
    Stats::open($statsFile);
}

$run = [
    'jobs' => $jobs,
    'php' => $php,
    'only' => $only,
    'extensions' => $extensions,
    'zendMm' => $zendMm,
    'sandbox' => $sandbox,
    'manifest' => $plan['manifest'],
    'skip' => $skip,
    'cache' => $cacheDir,
    'recorder' => recorder_hash(),
    'binary' => (string) (@filesize((string) realpath($php)) ?: 0) . '/' . (string) (@filemtime((string) realpath($php)) ?: 0),
    'signatures' => $plan['signatures'],
];
$passes = [];
for ($pass = 0; $pass < ($verify ? 2 : 1); $pass++) {
    $passes[] = run_units($plan['units'], $plan['weights'], $run);
}
[$unitLines, $failed] = $passes[0];
$differing = [];
if ($verify) {
    [$second, $lostSecond] = $passes[1];
    $failed = array_merge($failed, $lostSecond);
    foreach (array_keys($plan['units']) as $index) {
        if (!isset($unitLines[$index]) || !isset($second[$index])) {
            $unitLines[$index] = $unitLines[$index] ?? $second[$index] ?? [];
            continue;
        }
        [$rows, $differs] = reconcile($unitLines[$index], $second[$index]);
        if ($differs) {
            $differing[] = implode(' ', $plan['units'][$index]);
        }
        $unitLines[$index] = array_fill_keys($rows, true);
    }
}
Stats::close((int) array_sum($plan['weights']));

$rows = [];
foreach ($unitLines as $lines) {
    foreach (array_keys($lines) as $line) {
        $rows[$line] = true;
    }
}
$measured = [];
foreach (array_keys($rows) as $row) {
    [$builtin, , $kind] = explode("\t", $row);
    if ($kind === 'probed') {
        $measured[$builtin] = true;
    }
}
$asserted = [];
foreach ($plan['nothing'] as $function) {
    if (!isset($measured[$function])) {
        $rows["$function\t\tprobed\t"] = true;
        $asserted[] = $function;
    }
}
$rows = array_keys($rows);
foreach ($failed as $unit) {
    fwrite(STDERR, 'lost twice, its rows are missing: ' . implode(' ', $unit) . "\n");
}
foreach ($differing as $unit) {
    fwrite(STDERR, "the two runs differ: $unit\n");
}

if ($failed === [] && ($out !== null || $update !== null)) {
    $target = (string) ($out ?? $update);
    $final = $update === null ? $rows : merge_rows(read_rows($update), $rows, $wanted);
    $counts = [
        'units' => count($plan['units']),
        'forks' => (int) array_sum($plan['weights']),
        'timeouts' => Stats::failures()[0],
        'died' => Stats::failures()[1],
        'differing' => count($differing),
    ];
    $runInfo = [
        'selection' => ['only' => $only, 'extensions' => $extensions],
        'flags' => array_values(array_filter($argv, static fn (string $argument): bool => str_starts_with($argument, '--'))),
        'totals' => $counts,
    ];
    write_atomic($target, table_text($final));
    $probedNames = [];
    foreach ($final as $row) {
        [$builtin, , $kind] = explode("\t", $row);
        if ($kind === 'probed') {
            $probedNames[$builtin] = true;
        }
    }
    // The stub declarations of the recorded extensions that have no row: the drift test reads them.
    $gaps = [];
    foreach (coverage_lines($probedNames, [], $wanted, $asserted) as $line) {
        [$section, $extension, $name, $reason] = explode("\t", $line);
        if ($section === 'stub' && $reason === 'loaded, no row') {
            $gaps[] = "$extension\t$name";
        }
    }
    write_atomic(sidecar_path($target), sidecar_text($final, $plan['manifest'], $run['recorder'], $skip, $sandboxMode, $gaps, $runInfo, earlier_runs($target)));
} elseif ($failed === [] && $out === null && $update === null) {
    echo table_text($rows);
}

if ($coverage !== null && $failed === []) {
    $probed = [];
    foreach ($rows as $row) {
        [$builtin, , $kind] = explode("\t", $row);
        if ($kind === 'probed') {
            $probed[$builtin] = true;
        }
    }
    write_atomic($coverage, implode("\n", coverage_lines($probed, [], $wanted, $asserted)) . "\n");
}

exit($failed === [] ? 0 : 1);
