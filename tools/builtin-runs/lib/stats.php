<?php
// What a run measures about itself: each child's wall time and outcome, each
// worker's peak memory, and the counts the progress line shows.

final class Stats
{
    /** Whether a line is kept for every child (`--stats`). */
    public static bool $enabled = false;
    /** The kind of the unit being run. */
    public static string $unit = '';
    /** @var array{ms: float, outcome: string}|null the last child */
    public static ?array $last = null;
    /** @var list<array{string, string, string, float, string}> children not yet sent to the parent */
    public static array $lines = [];
    public static int $children = 0;
    public static float $ms = 0.0;
    public static int $timeouts = 0;
    public static int $died = 0;

    /** Upper bounds, in milliseconds, of the buckets of the wall-time histogram. */
    private const BUCKETS = [1, 2, 5, 10, 20, 50, 100, 500, 1000, 3000];

    /** @var resource|null */
    private static $file = null;
    private static string $path = '';
    /** @var array<int, array{peak: int, rss: int, children: int, ms: float, timeouts: int, died: int}> */
    private static array $workers = [];
    /** @var array<string, int> */
    private static array $histogram = [];
    /** @var list<array{string, string, string, float, string}> */
    private static array $slowest = [];
    /** @var array<string, int> */
    private static array $timeoutsByClass = [];
    private static float $started = 0.0;
    private static string $loadStart = '';

    /** Notes a finished child. */
    public static function note(float $ms, string $outcome): void
    {
        self::$last = ['ms' => $ms, 'outcome' => $outcome];
        self::$children++;
        self::$ms += $ms;
        if ($outcome === 'timeout') {
            self::$timeouts++;
        } elseif ($outcome === 'died') {
            self::$died++;
        }
    }

    /** The line for the child just noted, kept when `--stats` asked for it. */
    public static function child(string $builtin, string $where): void
    {
        if (self::$enabled && self::$last !== null) {
            self::$lines[] = [self::$unit, $builtin, $where, self::$last['ms'], self::$last['outcome']];
        }
    }

    /**
     * What a worker tells the parent after a unit: the children since the
     * last time, and its totals so far.
     *
     * @return array{lines: list<array{string, string, string, float, string}>, peak: int, rss: int, children: int, ms: float, timeouts: int, died: int}
     */
    public static function payload(): array
    {
        $payload = [
            'lines' => self::$lines,
            'peak' => memory_get_peak_usage(true),
            'rss' => self::peakResident(),
            'children' => self::$children,
            'ms' => self::$ms,
            'timeouts' => self::$timeouts,
            'died' => self::$died,
        ];
        self::$lines = [];
        return $payload;
    }

    /** The most memory this process has had resident, in bytes (what `peak_mb` cannot say with the allocator off). */
    private static function peakResident(): int
    {
        foreach (@file('/proc/self/status', FILE_IGNORE_NEW_LINES) ?: [] as $line) {
            if (str_starts_with($line, 'VmHWM:')) {
                return 1024 * (int) substr($line, 6);
            }
        }
        return 0;
    }

    /** Takes in what worker `$pid` sent. */
    public static function absorb(int $pid, mixed $payload): void
    {
        if (!is_array($payload)) {
            return;
        }
        self::$workers[$pid] = [
            'peak' => $payload['peak'],
            'rss' => $payload['rss'],
            'children' => $payload['children'],
            'ms' => $payload['ms'],
            'timeouts' => $payload['timeouts'],
            'died' => $payload['died'],
        ];
        if (self::$file === null) {
            return;
        }
        foreach ($payload['lines'] as $line) {
            self::write($line);
        }
    }

    /** @return array{int, int} the timeouts and the children that died, over all workers */
    public static function failures(): array
    {
        $timeouts = $died = 0;
        foreach (self::$workers as $worker) {
            $timeouts += $worker['timeouts'];
            $died += $worker['died'];
        }
        return [$timeouts, $died];
    }

    /** Starts the `--stats` file: written beside its name and renamed when the run ends. */
    public static function open(string $path): void
    {
        $dir = dirname($path);
        $free = @disk_free_space($dir);
        if ($free === false || $free < 2 * 1024 ** 3) {
            fwrite(STDERR, "less than 2 GiB free in $dir\n");
            exit(1);
        }
        $file = @fopen($path . '.tmp', 'w');
        if ($file === false) {
            fwrite(STDERR, "cannot write $path.tmp\n");
            exit(1);
        }
        self::$file = $file;
        self::$path = $path;
        self::$enabled = true;
        self::$started = microtime(true);
        self::$loadStart = trim((string) @file_get_contents('/proc/loadavg'));
    }

    /** @param array{string, string, string, float, string} $line */
    private static function write(array $line): void
    {
        [$kind, $builtin, $where, $ms, $outcome] = $line;
        foreach (self::BUCKETS as $bound) {
            if ($ms < $bound) {
                $key = "<{$bound}ms";
                break;
            }
        }
        $key ??= '>=' . self::BUCKETS[count(self::BUCKETS) - 1] . 'ms';
        self::$histogram[$key] = (self::$histogram[$key] ?? 0) + 1;
        if ($outcome === 'timeout') {
            $class = str_contains($builtin, '::') ? explode('::', $builtin)[0] : '(function)';
            self::$timeoutsByClass[$class] = (self::$timeoutsByClass[$class] ?? 0) + 1;
        }
        self::$slowest[] = $line;
        if (count(self::$slowest) > 64) {
            usort(self::$slowest, static fn (array $a, array $b): int => $b[3] <=> $a[3]);
            self::$slowest = array_slice(self::$slowest, 0, 20);
        }
        self::put(['t' => 'child', 'kind' => $kind, 'builtin' => $builtin, 'where' => $where, 'ms' => round($ms, 2), 'outcome' => $outcome]);
    }

    /** @param array<string, mixed> $record */
    private static function put(array $record): void
    {
        $text = json_encode($record, JSON_UNESCAPED_SLASHES | JSON_INVALID_UTF8_SUBSTITUTE) . "\n";
        if (self::$file === null || @fwrite(self::$file, $text) !== strlen($text)) {
            fwrite(STDERR, "cannot write " . self::$path . ".tmp\n");
            exit(1);
        }
    }

    /** Writes the summary and puts the file in place. */
    public static function close(int $forks): void
    {
        $file = self::$file;
        if ($file === null) {
            return;
        }
        usort(self::$slowest, static fn (array $a, array $b): int => $b[3] <=> $a[3]);
        $workers = [];
        foreach (self::$workers as $pid => $worker) {
            $workers[] = [
                'pid' => $pid,
                'peak_mb' => round($worker['peak'] / 1048576, 1),
                'rss_mb' => round($worker['rss'] / 1048576, 1),
                'children' => $worker['children'],
                'mean_child_ms' => $worker['children'] > 0 ? round($worker['ms'] / $worker['children'], 2) : 0,
            ];
            if ($worker['peak'] > 32 * 1048576) {
                fwrite(STDERR, "worker $pid peaked at " . round($worker['peak'] / 1048576) . " MB\n");
            }
        }
        [$timeouts, $died] = self::failures();
        self::put([
            't' => 'summary',
            'forks' => $forks,
            'wall_s' => round(microtime(true) - self::$started, 1),
            'timeouts' => $timeouts,
            'died' => $died,
            'histogram' => self::$histogram,
            'slowest' => array_map(
                static fn (array $line): array => ['builtin' => $line[1], 'where' => $line[2], 'ms' => round($line[3], 1), 'outcome' => $line[4]],
                array_slice(self::$slowest, 0, 20),
            ),
            'timeouts_by_class' => self::$timeoutsByClass,
            'workers' => $workers,
            'load_start' => self::$loadStart,
            'load_end' => trim((string) @file_get_contents('/proc/loadavg')),
            'free_bytes' => @disk_free_space(dirname(self::$path)),
        ]);
        if (!fflush($file) || !fclose($file) || !rename(self::$path . '.tmp', self::$path)) {
            fwrite(STDERR, "cannot write " . self::$path . "\n");
            exit(1);
        }
        self::$file = null;
    }
}

/** The progress line: the share of the planned forks done, the time, and the failures. */
function progress_line(int $done, int $total, float $started): string
{
    $elapsed = microtime(true) - $started;
    $rate = $elapsed > 0 ? $done / $elapsed : 0.0;
    $eta = $rate > 0 ? ($total - $done) / $rate : 0.0;
    [$timeouts, $died] = Stats::failures();
    return sprintf(
        '%5.1f%% %d/%d forks  %s elapsed  eta %s  %d forks/s  timeouts %d  died %d',
        $total > 0 ? 100 * $done / $total : 100.0,
        $done,
        $total,
        clock($elapsed),
        clock($eta),
        $rate,
        $timeouts,
        $died,
    );
}

/** Seconds as `1h02m03s`, `4m05s` or `7s`. */
function clock(float $seconds): string
{
    $seconds = (int) round($seconds);
    return match (true) {
        $seconds >= 3600 => sprintf('%dh%02dm%02ds', intdiv($seconds, 3600), intdiv($seconds % 3600, 60), $seconds % 60),
        $seconds >= 60 => sprintf('%dm%02ds', intdiv($seconds, 60), $seconds % 60),
        default => $seconds . 's',
    };
}
