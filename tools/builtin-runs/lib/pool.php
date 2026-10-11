<?php
// The workers and the queue of units they take from.

/** Removes a directory and what is in it. */
function remove_directory(string $dir): void
{
    chdir('/');
    if (!is_dir($dir)) {
        return;
    }
    $entries = new RecursiveIteratorIterator(
        new RecursiveDirectoryIterator($dir, FilesystemIterator::SKIP_DOTS),
        RecursiveIteratorIterator::CHILD_FIRST,
    );
    foreach ($entries as $entry) {
        $entry->isDir() && !$entry->isLink() ? rmdir($entry->getPathname()) : unlink($entry->getPathname());
    }
    rmdir($dir);
}

/** A message on a socket: one line. */
function send(mixed $socket, mixed $message): void
{
    fwrite($socket, base64_encode(serialize($message)) . "\n");
}

/** The message on a socket, or `null` when it was closed. */
function receive(mixed $socket): mixed
{
    $line = fgets($socket);
    if ($line === false) {
        return null;
    }
    return unserialize(base64_decode(trim($line)), ['allowed_classes' => false]);
}

/**
 * A worker: takes units from the parent one at a time, in a directory of
 * its own, and sends back the lines each found.
 */
function work(mixed $socket, callable $wanted): never
{
    $dir = sys_get_temp_dir() . '/builtin-runs-' . getmypid();
    @mkdir($dir);
    chdir($dir);
    send($socket, ['ready', []]);
    while (is_array($unit = receive($socket))) {
        send($socket, ['done', run_unit($unit, $wanted), Stats::payload()]);
    }
    remove_directory($dir);
    exit(0);
}

/**
 * Runs the units on `$jobs` workers: each takes the next unit from the front
 * of the queue when it has finished its last.
 *
 * @param list<array{string, string, string, int, int}> $units
 * @param list<int> $weights the forks each unit makes, for the progress line
 * @return array<string, true>
 */
function run_units(array $units, int $jobs, callable $wanted, array $weights): array
{
    $out = [];
    $sockets = [];
    for ($i = 0; $i < $jobs; $i++) {
        $pair = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP);
        $pid = pcntl_fork();
        if ($pid === -1) {
            fwrite(STDERR, "fork failed\n");
            exit(1);
        }
        if ($pid === 0) {
            fclose($pair[0]);
            foreach ($sockets as $other) {
                fclose($other);
            }
            work($pair[1], $wanted);
        }
        fclose($pair[1]);
        $sockets[$pid] = $pair[0];
    }
    $next = 0;
    $total = count($units);
    $planned = (int) array_sum($weights);
    $done = 0;
    $running = [];
    $started = microtime(true);
    $shown = $started;
    while ($sockets !== []) {
        $read = array_values($sockets);
        $write = $except = null;
        if (stream_select($read, $write, $except, 5) === false) {
            fwrite(STDERR, "select failed\n");
            exit(1);
        }
        foreach ($read as $socket) {
            $pid = array_search($socket, $sockets, true);
            $message = receive($socket);
            if (is_array($message)) {
                foreach (array_keys($message[1]) as $line) {
                    $out[$line] = true;
                }
                Stats::absorb((int) $pid, $message[2] ?? null);
                $done += $running[$pid] ?? 0;
            }
            if ($message !== null && $next < $total) {
                $running[$pid] = $weights[$next];
                send($socket, $units[$next++]);
                continue;
            }
            fclose($socket);
            unset($sockets[$pid]);
            pcntl_waitpid($pid, $status);
        }
        if (microtime(true) - $shown >= 5) {
            fwrite(STDERR, progress_line($done, $planned, $started) . "\n");
            $shown = microtime(true);
        }
    }
    return $out;
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
