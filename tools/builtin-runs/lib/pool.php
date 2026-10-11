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
 * units, the children each makes, the functions that run nothing, and what
 * that PHP is.
 *
 * @return array{manifest: array{php: string, extensions: list<string>, ffi: bool}, units: list<array{string, string, string, int, int}>, weights: list<int>, nothing: list<string>}
 */
function plan_run(string $php, ?string $only, bool $zendMm): array
{
    $worker = start_worker($php, $zendMm);
    try {
        send($worker->in, ['init', $only, false, 'off']);
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
    /** @var array{manifest: array{php: string, extensions: list<string>, ffi: bool}, units: list<array{string, string, string, int, int}>, weights: list<int>, nothing: list<string>} $result */
    $result = ['manifest' => $ready[1], 'units' => $plan[1], 'weights' => $plan[2], 'nothing' => $plan[3]];
    return $result;
}

/**
 * Runs the units on `$jobs` workers: each takes the next unit from the front
 * of the queue when it has finished its last. A worker that dies has its
 * unit run again on another; a unit lost twice is given back in the second
 * element.
 *
 * @param list<array{string, string, string, int, int}> $units
 * @param list<int> $weights the forks each unit makes, for the progress line
 * @param array{php: string, extensions: list<string>, ffi: bool} $manifest what the workers must be
 * @param string $skip which children are not run (`off` or `dead`)
 * @return array{array<string, true>, list<array{string, string, string, int, int}>}
 */
function run_units(array $units, array $weights, int $jobs, string $php, ?string $only, bool $zendMm, array $manifest, string $skip): array
{
    $out = [];
    $failed = [];
    $total = count($units);
    $planned = (int) array_sum($weights);
    $next = 0;
    $again = [];
    $tries = [];
    $done = 0;
    $respawns = 0;
    $started = microtime(true);
    $shown = $started;
    $spawn = static function () use ($php, $only, $zendMm, $skip): void {
        $worker = start_worker($php, $zendMm);
        try {
            send($worker->in, ['init', $only, Stats::$enabled, $skip]);
        } catch (RuntimeException) {
        }
    };
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
                    fail("a worker of $php did not start");
                }
                $index = $worker->unit;
                wipe_directory(Workdir::of($worker->pid));
                release_worker($worker);
                if ($index !== null) {
                    $tries[$index] = ($tries[$index] ?? 0) + 1;
                    if ($tries[$index] < 2) {
                        $again[] = $index;
                    } else {
                        $failed[] = $units[$index];
                        $done += $weights[$index];
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
                if ($message[1] !== $manifest) {
                    fail('a worker is not the PHP that was planned with: ' . json_encode([$message[1], $manifest]));
                }
                $worker->ready = true;
            } elseif ($message[0] === 'done') {
                foreach (array_keys($message[1]) as $line) {
                    $out[$line] = true;
                }
                Stats::absorb($worker->pid, $message[2] ?? null);
                $done += $weights[$worker->unit ?? 0] ?? 0;
                $worker->unit = null;
            }
            $index = array_shift($again);
            if ($index === null && $next < $total) {
                $index = $next++;
            }
            if ($index === null) {
                release_worker($worker);
                continue;
            }
            $worker->unit = $index;
            try {
                send($worker->in, ['unit', $units[$index]]);
            } catch (RuntimeException) {
                // Seen as a death on the next read.
            }
        }
        if (microtime(true) - $shown >= 5) {
            fwrite(STDERR, progress_line($done, $planned, $started) . "\n");
            $shown = microtime(true);
        }
    }
    return [$out, $failed];
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
