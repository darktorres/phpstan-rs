<?php
// The command that starts a worker, and the size of the scratch space each
// gets. Under `bwrap` a worker sees the host's files read-only, a private home
// directory of its own that is a capped tmpfs, and no network; it is the head
// of its own pid, uts and ipc namespaces.

/** The memory available to the machine, in bytes (`MemAvailable` in /proc/meminfo). */
function memory_available(): int
{
    foreach (@file('/proc/meminfo') ?: [] as $line) {
        if (preg_match('/^MemAvailable:\s+(\d+) kB/', $line, $match) === 1) {
            return (int) $match[1] * 1024;
        }
    }
    return 0;
}

/**
 * The size of each worker's scratch space, in bytes: 256 MiB, or less when the memory
 * available cannot hold every worker's at twice that. A worker's children
 * write at most 64 MiB each, so less than 256 MiB is refused by the run.
 */
function sandbox_cap(int $jobs): int
{
    return min(256 * 1024 ** 2, intdiv(memory_available(), 2 * max(1, $jobs)));
}

/**
 * The command that starts a worker.
 *
 * `bwrap`: the environment is cleared and set again; `sudo-unshare` runs the
 * worker in a network namespace of its own, as root's `unshare`, with the
 * privileges of the user, and shares the host's files (chosen only by
 * `--sandbox=sudo-unshare`, since it runs with root's rights for a moment);
 * `none` is the PHP itself.
 *
 * @param array{mode: string, cap: int} $sandbox
 * @param list<string>|null $modules the modules the worker loads beyond the base ones, and no others; `null` is the PHP's own configuration
 * @return list<string>
 */
function worker_command(string $php, bool $zendMm, array $sandbox, ?array $modules = null): array
{
    $lean = $modules === null ? [] : lean_arguments($php, $modules);
    $interpreter = [$php, ...$lean, '-d', 'ffi.enable=1', '-d', 'display_errors=0', '-d', 'memory_limit=512M', dirname(__DIR__) . '/worker.php'];
    // A `bwrap` worker's home is its scratch space; the other modes keep the caller's.
    $home = $sandbox['mode'] === 'bwrap' ? '/home/worker' : (string) getenv('HOME');
    $variables = ['PATH' => (string) getenv('PATH'), 'HOME' => $home, 'LC_ALL' => 'C', 'TZ' => 'UTC'];
    if (!$zendMm) {
        $variables['USE_ZEND_ALLOC'] = '0';
    }
    if ($sandbox['mode'] === 'bwrap') {
        $command = ['bwrap', '--unshare-net', '--unshare-pid', '--unshare-uts', '--unshare-ipc', '--hostname', 'rec', '--die-with-parent', '--new-session', '--clearenv'];
        foreach ($variables as $name => $value) {
            array_push($command, '--setenv', $name, $value);
        }
        // /run holds the host's listening sockets (Docker, containerd, the system bus, MySQL); a
        // network namespace does not cut those off, so /run is hidden as /home is.
        array_push($command, '--ro-bind', '/', '/', '--dev', '/dev', '--proc', '/proc', '--tmpfs', '/home', '--tmpfs', '/run');
        $tools = dirname(__DIR__);
        array_push($command, '--ro-bind', $tools, $tools);
        $stubs = phpstorm_stubs_dir();
        array_push($command, '--ro-bind', $stubs, $stubs);
        $phpDir = dirname((string) realpath($php));
        if (str_starts_with($phpDir, '/home/')) {
            array_push($command, '--ro-bind', $phpDir, $phpDir);
        }
        array_push($command, '--size', (string) $sandbox['cap'], '--tmpfs', '/home/worker', '--chdir', '/home/worker');
        return [...$command, ...$interpreter];
    }
    if ($sandbox['mode'] === 'sudo-unshare') {
        $assignments = [];
        foreach ($variables as $name => $value) {
            $assignments[] = "$name=$value";
        }
        return [
            'sudo', '-n', 'unshare', '-n', '--',
            'setpriv', '--reuid=' . posix_getuid(), '--regid=' . posix_getgid(), '--clear-groups', '--',
            'env', '-i', ...$assignments, ...$interpreter,
        ];
    }
    return $interpreter;
}
