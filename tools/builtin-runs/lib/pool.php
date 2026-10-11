<?php
// The orchestrator's side: workers started as PHP processes of their own, and
// the queue of units they take from.

final class Worker
{
    public int $pid;
    /** The index of the unit it is running. */
    public ?int $unit = null;
    public bool $ready = false;

    /**
     * @param resource $process
     * @param resource $in what it reads
     * @param resource $out what it writes
     */
    public function __construct(public $process, public $in, public $out)
    {
        $this->pid = (int) (proc_get_status($process)['pid'] ?? 0);
    }
}

final class Pool
{
    /** @var array<int, Worker> */
    public static array $workers = [];

    /** Ends every worker at once and removes their directories; nothing is written. */
    public static function abandon(): void
    {
        foreach (self::$workers as $worker) {
            proc_terminate($worker->process, SIGKILL);
            wipe_directory(Workdir::of($worker->pid));
        }
        self::$workers = [];
    }
}

/** Stops the run with a message. */
function fail(string $message): never
{
    Pool::abandon();
    fwrite(STDERR, $message . "\n");
    exit(1);
}

/**
 * Starts a worker: a fresh PHP, with nothing on its standard streams and the
 * protocol on descriptors 3 and 4.
 */
function start_worker(string $php, bool $zendMm): Worker
{
    $env = getenv();
    if (!$zendMm) {
        $env['USE_ZEND_ALLOC'] = '0';
    }
    $command = [$php, '-d', 'ffi.enable=1', '-d', 'display_errors=0', '-d', 'memory_limit=512M', dirname(__DIR__) . '/worker.php'];
    $descriptors = [
        0 => ['file', '/dev/null', 'r'],
        1 => ['file', '/dev/null', 'w'],
        2 => ['file', '/dev/null', 'w'],
        3 => ['pipe', 'r'],
        4 => ['pipe', 'w'],
    ];
    $process = proc_open($command, $descriptors, $pipes, null, $env);
    if (!is_resource($process)) {
        fail("cannot start $php");
    }
    $worker = new Worker($process, $pipes[3], $pipes[4]);
    Pool::$workers[$worker->pid] = $worker;
    return $worker;
}

/** Lets a worker go: it ends when its input closes. */
function release_worker(Worker $worker): void
{
    unset(Pool::$workers[$worker->pid]);
    @fclose($worker->in);
    @fclose($worker->out);
    proc_close($worker->process);
}

/**
 * Asks a worker of the PHP that is recorded what there is to probe: the
 * units, the children each makes, the functions that run nothing, the
 * signatures of the units, and what that PHP is.
 *
 * @param list<string>|null $extensions
 * @return array{manifest: array{php: string, extensions: array<string, array{version: ?string, info: ?string, provides: list<string>}>, ffi: bool}, units: list<array{string, string, string, int, int}>, weights: list<int>, nothing: list<string>, signatures: list<string>}
 */
function plan_run(string $php, ?string $only, ?array $extensions, bool $zendMm): array
{
    $worker = start_worker($php, $zendMm);
    try {
        send($worker->in, ['init', $only, false, 'off', $extensions]);
        $ready = receive($worker->out);
        if (!is_array($ready) || $ready[0] !== 'ready') {
            fail("$php did not start a worker");
        }
        send($worker->in, ['plan']);
        $plan = receive($worker->out);
        if (!is_array($plan) || $plan[0] !== 'plan') {
            fail("$php could not plan the run");
        }
    } catch (RuntimeException) {
        fail("$php did not start a worker");
    }
    release_worker($worker);
    /** @var array{manifest: array{php: string, extensions: array<string, array{version: ?string, info: ?string, provides: list<string>}>, ffi: bool}, units: list<array{string, string, string, int, int}>, weights: list<int>, nothing: list<string>, signatures: list<string>} $result */
    $result = ['manifest' => $ready[1], 'units' => $plan[1], 'weights' => $plan[2], 'nothing' => $plan[3], 'signatures' => $plan[4]];
    return $result;
}

/**
 * The `probed` builtins of each class, keyed by the class's lowercased name
 * and sorted, from the lines of the units that are done.
 *
 * @param array<int, array<string, true>> $unitLines
 * @return array<string, array<string, int>>
 */
function probed_by_class(array $unitLines): array
{
    $known = [];
    foreach ($unitLines as $lines) {
        foreach (array_keys($lines) as $line) {
            [$builtin, , $kind] = explode("\t", $line);
            if ($kind === 'probed' && str_contains($builtin, '::')) {
                $known[explode('::', $builtin, 2)[0]][$builtin] = 1;
            }
        }
    }
    foreach (array_keys($known) as $class) {
        ksort($known[$class]);
    }
    ksort($known);
    return $known;
}

/**
 * Runs the units on `$run['jobs']` workers, each taking the next unit from the
 * queue when it has finished its last. The units that are not held come first,
 * and a held unit is sent only once they are all done, with the `probed`
 * builtins of its class, which the released rule reads. Units whose result
 * the cache holds are not run. A worker that dies has its unit run again on
 * another; a unit lost twice is given back in the second element.
 *
 * @param list<array{string, string, string, int, int}> $units
 * @param list<int> $weights the forks each unit makes, for the progress line
 * @param array{jobs: int, php: string, only: ?string, extensions: ?list<string>, zendMm: bool, manifest: array{php: string, extensions: array<string, array{version: ?string, info: ?string, provides: list<string>}>, ffi: bool}, skip: string, cache: ?string, recorder: string, binary: string, signatures: list<string>} $run
 * @return array{array<int, array<string, true>>, list<array{string, string, string, int, int}>} the lines of each unit by its index, and the units lost twice
 */
function run_units(array $units, array $weights, array $run): array
{
    $unitLines = [];
    $failed = [];
    $tries = [];
    $done = 0;
    $respawns = 0;
    $jobs = $run['jobs'];
    $manifestHash = sha1((string) json_encode($run['manifest']));
    $planned = (int) array_sum($weights);
    $started = microtime(true);
    $shown = $started;
    $known = null;
    $idle = [];
    $next = 0;
    $again = [];
    $order = [];
    foreach ([false, true] as $held) {
        foreach ($units as $i => $unit) {
            if (($unit[0] === 'held') === $held) {
                $order[] = $i;
            }
        }
    }
    $keyOf = static function (int $index) use ($units, $run, $manifestHash, &$known): ?string {
        if ($run['cache'] === null) {
            return null;
        }
        $unit = $units[$index];
        return cache_key([
            'recorder' => $run['recorder'],
            'binary' => $run['binary'],
            'manifest' => $manifestHash,
            'unit' => $unit,
            'signature' => $run['signatures'][$index],
            'skip' => $run['skip'],
            'sandbox' => 'none',
            'known' => $unit[0] === 'held' ? ($known[strtolower($unit[1])] ?? []) : null,
        ]);
    };
    // The stored rows of a unit, or `null` when there are none.
    $stored = static function (int $index) use ($run, $keyOf): ?array {
        $key = $keyOf($index);
        return $key === null || $run['cache'] === null ? null : cache_read($run['cache'], $key);
    };
    $complete = static function (int $index, array $rows) use (&$unitLines, &$done, $weights): void {
        $unitLines[$index] = array_fill_keys($rows, true);
        $done += $weights[$index];
    };
    $spawn = static function () use ($run): void {
        $worker = start_worker($run['php'], $run['zendMm']);
        try {
            send($worker->in, ['init', $run['only'], Stats::$enabled, $run['skip'], $run['extensions']]);
        } catch (RuntimeException) {
        }
    };
    $open = 0;
    // Gives a ready worker its next unit, or keeps it idle while a held unit
    // waits for the others, or releases it when there is nothing left.
    $assign = static function (Worker $worker) use (&$next, &$again, &$open, &$known, &$idle, &$order, &$total, $units): void {
        if ($again === [] && $next < $total && $units[$order[$next]][0] === 'held' && $open > 0) {
            $idle[$worker->pid] = $worker;
            return;
        }
        if ($again !== []) {
            $index = array_shift($again);
        } elseif ($next < $total) {
            $index = $order[$next++];
        } else {
            release_worker($worker);
            return;
        }
        $unit = $units[$index];
        $worker->unit = $index;
        try {
            send($worker->in, ['unit', $unit, $unit[0] === 'held' ? ($known[strtolower($unit[1])] ?? []) : []]);
        } catch (RuntimeException) {
            // Seen as a death on the next read.
        }
    };
    // Called once the units that are not held are all done: the held units
    // that the cache holds are completed here, the others are queued.
    $lift = static function () use (&$known, &$idle, &$unitLines, &$order, &$next, &$total, $assign, $complete, $stored): void {
        $known = probed_by_class($unitLines);
        $queued = array_slice($order, 0, $next);
        foreach (array_slice($order, $next) as $index) {
            $rows = $stored($index);
            if ($rows === null) {
                $queued[] = $index;
            } else {
                $complete($index, $rows);
            }
        }
        $order = $queued;
        $total = count($order);
        $waiting = $idle;
        $idle = [];
        foreach ($waiting as $worker) {
            if (isset(Pool::$workers[$worker->pid])) {
                $assign($worker);
            }
        }
    };
    $total = 0;
    // The units with a stored result, of the ones that are not held, are
    // completed before any worker starts.
    $queued = [];
    foreach ($order as $index) {
        $rows = $units[$index][0] === 'held' ? null : $stored($index);
        if ($rows === null) {
            $queued[] = $index;
        } else {
            $complete($index, $rows);
        }
    }
    $order = $queued;
    $total = count($order);
    $open = count(array_filter($order, static fn (int $index): bool => $units[$index][0] !== 'held'));
    if ($open === 0) {
        $lift();
    }
    for ($i = 0; $i < min($jobs, max(1, $total)); $i++) {
        $spawn();
    }
    while (Pool::$workers !== []) {
        $read = array_map(static fn (Worker $worker) => $worker->out, Pool::$workers);
        $write = $except = null;
        if (stream_select($read, $write, $except, 5) === false) {
            fail('select failed');
        }
        foreach ($read as $stream) {
            $worker = null;
            foreach (Pool::$workers as $candidate) {
                if ($candidate->out === $stream) {
                    $worker = $candidate;
                }
            }
            if ($worker === null) {
                continue;
            }
            $message = receive($stream);
            if (!is_array($message)) {
                // The worker died. A unit it was running is run again once.
                if (!$worker->ready) {
                    fail("a worker of {$run['php']} did not start");
                }
                $index = $worker->unit;
                wipe_directory(Workdir::of($worker->pid));
                release_worker($worker);
                unset($idle[$worker->pid]);
                if ($index !== null) {
                    $tries[$index] = ($tries[$index] ?? 0) + 1;
                    if ($tries[$index] < 2) {
                        $again[] = $index;
                    } else {
                        $failed[] = $units[$index];
                        $done += $weights[$index];
                        if ($units[$index][0] !== 'held' && --$open === 0) {
                            $lift();
                        }
                    }
                }
                if ($again !== [] || $next < $total) {
                    if (++$respawns > 3 * $jobs + 20) {
                        fail('workers keep dying');
                    }
                    $spawn();
                }
                continue;
            }
            if ($message[0] === 'ready') {
                if ($message[1] !== $run['manifest']) {
                    fail('a worker is not the PHP that was planned with: ' . json_encode([$message[1], $run['manifest']]));
                }
                $worker->ready = true;
            } elseif ($message[0] === 'done') {
                $index = $worker->unit ?? 0;
                $unitLines[$index] = $message[1];
                $key = $keyOf($index);
                if ($key !== null && $run['cache'] !== null) {
                    cache_write($run['cache'], $key, array_values(array_map(strval(...), array_keys($message[1]))));
                }
                Stats::absorb($worker->pid, $message[2] ?? null);
                $done += $weights[$index] ?? 0;
                $worker->unit = null;
                if ($units[$index][0] !== 'held' && --$open === 0) {
                    $lift();
                }
            }
            $assign($worker);
        }
        if (microtime(true) - $shown >= 5) {
            fwrite(STDERR, progress_line($done, $planned, $started) . "\n");
            $shown = microtime(true);
        }
    }
    /** @var array{array<int, array<string, true>>, list<array{string, string, string, int, int}>} $result */
    $result = [$unitLines, $failed];
    return $result;
}

/**
 * The value of the option `--$name=value`, or `null` when it was not given.
 *
 * @param array<int|string, mixed> $argv
 */
function option(array $argv, string $name): ?string
{
    foreach ($argv as $argument) {
        if (is_string($argument) && str_starts_with($argument, "--$name=")) {
            return substr($argument, strlen($name) + 3);
        }
    }
    return null;
}

/**
 * The number of workers asked for, or one for each processor.
 *
 * @param array<int|string, mixed> $argv
 */
function jobs(array $argv): int
{
    $asked = option($argv, 'jobs');
    if ($asked !== null) {
        return max(1, (int) $asked);
    }
    return max(1, (int) shell_exec('nproc 2>/dev/null') ?: 1);
}
