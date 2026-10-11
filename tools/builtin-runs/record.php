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
// Each call runs in a child process of its own, in an empty directory, with
// an alarm, so that a call that ends the process, changes its state or
// blocks affects no other. Run it with the PHP whose behaviour is wanted:
//
//   php tools/builtin-runs/record.php > crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv
//
// A regular expression as the first argument limits it to the builtins
// whose lowercased name it matches. `--jobs=N` sets the number of workers
// (one for each processor by default): each takes the next function or
// method from a queue as it finishes its last, so the machine stays busy
// to the end.

declare(strict_types=0);

error_reporting(0);
ini_set('display_errors', '0');
ini_set('memory_limit', '512M');

if (!function_exists('pcntl_fork')) {
    fwrite(STDERR, "pcntl is needed\n");
    exit(1);
}

// A probe that aborts (a failed assertion in an extension, a fault) must not
// start the system's crash handler, which costs seconds for each: processes
// that are not dumpable never start it, and the setting is inherited by every
// worker and probe. It needs FFI, which PHP allows only when asked.
if (extension_loaded('ffi')) {
    if (ini_get('ffi.enable') !== '1' && getenv('BUILTIN_RUNS_REEXEC') === false) {
        putenv('BUILTIN_RUNS_REEXEC=1');
        pcntl_exec(PHP_BINARY, ['-d', 'ffi.enable=1', ...$argv]);
    }
    try {
        FFI::cdef('int prctl(int option, unsigned long a, unsigned long b, unsigned long c, unsigned long d);', 'libc.so.6')
            ->prctl(4, 0, 0, 0, 0);
    } catch (Throwable) {
    }
}

require_once __DIR__ . '/lib/seen.php';
require_once __DIR__ . '/lib/probes.php';
require_once __DIR__ . '/lib/child.php';
require_once __DIR__ . '/lib/plan.php';
require_once __DIR__ . '/lib/unit.php';
require_once __DIR__ . '/lib/pool.php';

$arguments = array_values(array_filter(array_slice($argv, 1), static fn (string $argument): bool => !str_starts_with($argument, '--')));
$only = $arguments[0] ?? null;
$wanted = static fn (string $builtin): bool => $only === null || preg_match($only, $builtin) === 1;

$out = run_units(units($wanted), jobs($argv), $wanted);

foreach (RUNS_NOTHING as $function) {
    if (function_exists($function) && $wanted($function)) {
        $out["$function\t\tprobed\t"] = true;
    }
}

$lines = array_keys($out);
sort($lines, SORT_STRING);
echo implode("\n", $lines), "\n";
