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
// whose lowercased name it matches. `--jobs=N` sets the number of workers
// (one for each processor by default): each takes the next function or
// method from a queue as it finishes its last, so the machine stays busy
// to the end. `--php=BIN` records the PHP BIN instead of this one, and the
// workers are its processes. `--zend-mm` leaves the workers on PHP's own
// allocator; by default they run with `USE_ZEND_ALLOC=0`, which gives the same
// output about twice as fast. `--skip=dead` first runs one child per held unit
// that makes the object and calls the method it is given the probe for; when
// that yields no object, none of the unit's children runs.
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
require_once __DIR__ . '/lib/pool.php';
require_once __DIR__ . '/lib/stats.php';

$arguments = array_values(array_filter(array_slice($argv, 1), static fn (string $argument): bool => !str_starts_with($argument, '--')));
$only = $arguments[0] ?? null;
$php = option($argv, 'php') ?? PHP_BINARY;
$zendMm = in_array('--zend-mm', $argv, true);
$skip = option($argv, 'skip') ?? 'off';
if (!in_array($skip, ['off', 'dead'], true)) {
    fwrite(STDERR, "--skip is off or dead\n");
    exit(2);
}

sweep_stale_directories();
pcntl_async_signals(true);
$stop = static function (int $signal): void {
    Pool::abandon();
    exit(128 + $signal);
};
pcntl_signal(SIGINT, $stop);
pcntl_signal(SIGTERM, $stop);

$plan = plan_run($php, $only, $zendMm);
if (in_array('--list', $argv, true)) {
    echo plan_summary($plan['units'], $plan['weights']);
    exit(0);
}
$statsFile = option($argv, 'stats');
if ($statsFile !== null) {
    Stats::open($statsFile);
}

[$out, $failed] = run_units($plan['units'], $plan['weights'], jobs($argv), $php, $only, $zendMm, $plan['manifest'], $skip);
Stats::close((int) array_sum($plan['weights']));

foreach ($plan['nothing'] as $function) {
    $out["$function\t\tprobed\t"] = true;
}
foreach ($failed as $unit) {
    fwrite(STDERR, 'lost twice, its rows are missing: ' . implode(' ', $unit) . "\n");
}

$lines = array_keys($out);
sort($lines, SORT_STRING);
echo implode("\n", $lines), "\n";
exit($failed === [] ? 0 : 1);
