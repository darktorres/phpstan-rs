<?php
// The servers that some probes need, started inside each worker's sandbox on Unix sockets, so no probe reaches
// the network. Each worker has a data directory of its own, bound into its sandbox and removed with it: the
// MySQL data is copied from a template, PostgreSQL is initialised, and Redis and the FTP server keep no state.
// The root is named by BUILTIN_RUNS_SERVERS. It holds `mysql-template/` (an initialised MySQL data directory),
// `ftp/` (a Python environment with pyftpdlib) and `ftp-server.py` (the FTP server the probes log in to).
// Unset, no server is started.

/** The root of the servers, or null when the environment does not name one. */
function servers_root(): ?string
{
    $root = getenv('BUILTIN_RUNS_SERVERS');
    return $root === false || $root === '' ? null : rtrim($root, '/');
}

/**
 * Makes the data directory of one worker's servers, with a copy of the MySQL template in it.
 *
 * @return string the directory, which the worker's sandbox binds read-write
 */
function servers_prepare(string $root): string
{
    $dir = $root . '/workers/' . bin2hex(random_bytes(6));
    mkdir($dir, 0700, true);
    copy_tree($root . '/mysql-template', $dir . '/mysql');
    return $dir;
}

/** Copies a directory tree, file by file, to a directory that does not exist yet. */
function copy_tree(string $from, string $to): void
{
    mkdir($to, 0700, true);
    foreach (new FilesystemIterator($from, FilesystemIterator::SKIP_DOTS) as $entry) {
        $target = $to . '/' . $entry->getFilename();
        if ($entry->isDir()) {
            copy_tree($entry->getPathname(), $target);
        } else {
            copy($entry->getPathname(), $target);
        }
    }
}

/**
 * The command that starts a worker with its servers: the boot script, the data directory, then the worker's own
 * command. The script exits into that command once the servers are up.
 *
 * @param list<string> $worker
 * @return list<string>
 */
function servers_command(string $root, string $dir, array $worker): array
{
    return ['sh', dirname(__DIR__) . '/lib/servers.sh', $root, $dir, ...$worker];
}
