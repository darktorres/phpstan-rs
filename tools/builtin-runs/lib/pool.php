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
        send($socket, ['done', run_unit($unit, $wanted)]);
    }
    remove_directory($dir);
    exit(0);
}

/**
 * Runs the units on `$jobs` workers: each takes the next unit from the front
 * of the queue when it has finished its last.
 *
 * @param list<array{string, string, string, int, int}> $units
 * @return array<string, true>
 */
function run_units(array $units, int $jobs, callable $wanted): array
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
    while ($sockets !== []) {
        $read = array_values($sockets);
        $write = $except = null;
        if (stream_select($read, $write, $except, null) === false) {
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
            }
            if ($message !== null && $next < $total) {
                send($socket, $units[$next++]);
                if ($next % 500 === 0) {
                    fwrite(STDERR, "$next / $total\n");
                }
                continue;
            }
            fclose($socket);
            unset($sockets[$pid]);
            pcntl_waitpid($pid, $status);
        }
    }
    return $out;
}

/** The number of workers asked for, or one for each processor. */
function jobs(array $argv): int
{
    foreach ($argv as $argument) {
        if (str_starts_with($argument, '--jobs=')) {
            return max(1, (int) substr($argument, 7));
        }
    }
    return max(1, (int) shell_exec('nproc 2>/dev/null') ?: 1);
}
