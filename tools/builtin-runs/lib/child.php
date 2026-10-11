<?php
// A call in a child process of its own.

/**
 * Runs `$call` in a child process and gives back what the probes saw, or
 * `null` when the child did not finish.
 *
 * @return array<string, true>|null
 */
function in_child(Closure $call): ?array
{
    $pair = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP);
    $pid = pcntl_fork();
    if ($pid === -1) {
        fwrite(STDERR, "fork failed\n");
        exit(1);
    }
    if ($pid === 0) {
        fclose($pair[0]);
        pcntl_alarm(3);
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
    fclose($pair[1]);
    $data = stream_get_contents($pair[0]);
    fclose($pair[0]);
    pcntl_waitpid($pid, $status);
    if ($data === '' || $data === false) {
        return null;
    }
    $seen = @unserialize($data, ['allowed_classes' => false]);
    return is_array($seen) ? $seen : null;
}
