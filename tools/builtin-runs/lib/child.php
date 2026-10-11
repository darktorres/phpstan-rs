<?php
// A call in a child process of its own.

/** The seconds a child may take before its alarm ends it. */
const CHILD_ALARM = 3;
/** The seconds after which the parent gives up on a child its alarm did not end. */
const CHILD_DEADLINE = 3.5;

/**
 * Runs `$call` in a child process, in a directory of its own, and gives
 * back what the probes saw, or `null` when the child did not finish. The
 * child is the head of a process group of its own, and the group is killed
 * once the child is reaped (or its deadline passes), so nothing it started
 * outlives it.
 *
 * @return array<string, true>|null
 */
function in_child(Closure $call): ?array
{
    $started = hrtime(true);
    $dir = Workdir::fresh();
    if ($dir === null) {
        Stats::note(0.0, 'died');
        return null;
    }
    $pair = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP);
    $pid = pcntl_fork();
    if ($pid === -1) {
        fwrite(STDERR, "fork failed\n");
        exit(1);
    }
    if ($pid === 0) {
        posix_setpgid(0, 0);
        fclose($pair[0]);
        Protocol::release();
        chdir($dir);
        pcntl_alarm(CHILD_ALARM);
        // What the child prints would mix with the parent's output.
        ob_start(static fn (string $buffer): string => '');
        try {
            $call();
        } catch (Throwable) {
        }
        @fwrite($pair[1], serialize(Seen::$entries));
        fclose($pair[1]);
        posix_kill(posix_getpid(), SIGKILL);
    }
    @posix_setpgid($pid, $pid);
    fclose($pair[1]);
    $data = read_until_closed($pair[0], microtime(true) + CHILD_DEADLINE);
    fclose($pair[0]);
    posix_kill(-$pid, SIGKILL);
    pcntl_waitpid($pid, $status);
    Workdir::discard($dir);
    $ms = (hrtime(true) - $started) / 1e6;
    if ($data === null || $data === '') {
        $alarmed = $data === null || (pcntl_wifsignaled($status) && pcntl_wtermsig($status) === SIGALRM);
        Stats::note($ms, $alarmed ? 'timeout' : 'died');
        return null;
    }
    $seen = @unserialize($data, ['allowed_classes' => false]);
    Stats::note($ms, is_array($seen) ? 'ok' : 'died');
    return is_array($seen) ? $seen : null;
}

/**
 * What a stream gives until its other end closes, or `null` when `$deadline`
 * (a time) passes first.
 *
 * @param resource $stream
 */
function read_until_closed($stream, float $deadline): ?string
{
    stream_set_blocking($stream, false);
    $data = '';
    while (true) {
        $left = $deadline - microtime(true);
        if ($left <= 0) {
            return null;
        }
        $read = [$stream];
        $write = $except = null;
        $ready = stream_select($read, $write, $except, (int) $left, (int) (($left - (int) $left) * 1e6));
        if ($ready === false) {
            return $data;
        }
        if ($ready === 0) {
            continue;
        }
        $chunk = fread($stream, 65536);
        if ($chunk === false || ($chunk === '' && feof($stream))) {
            return $data;
        }
        $data .= $chunk;
    }
}
