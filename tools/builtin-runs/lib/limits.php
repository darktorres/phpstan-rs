<?php
// The limits each child of a worker runs under: the address space, the size of
// a file it writes, the open files and the core. A worker that cannot set them
// does not run.

final class Limits
{
    private static ?FFI $libc = null;

    /** Loads the C library's `setrlimit`, and ignores SIGXFSZ, which children inherit. */
    public static function init(): void
    {
        if (!class_exists('FFI')) {
            fwrite(STDERR, "FFI is needed\n");
            exit(1);
        }
        self::$libc = FFI::cdef('struct rlimit { unsigned long rlim_cur; unsigned long rlim_max; }; int setrlimit(int resource, struct rlimit *rlim);', 'libc.so.6');
        pcntl_signal(SIGXFSZ, SIG_IGN);
    }

    /** Sets the limits in the calling process, a child, before it runs a call. */
    public static function apply(): void
    {
        $libc = self::$libc;
        if ($libc === null) {
            exit(1);
        }
        // RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_NOFILE, RLIMIT_CORE, RLIMIT_AS
        foreach ([[0, 60], [1, 64 * 1024 ** 2], [7, 1024], [4, 0], [9, 2 * 1024 ** 3]] as [$resource, $value]) {
            $limit = $libc->new('struct rlimit');
            $limit->rlim_cur = $value;
            $limit->rlim_max = $value;
            if ($libc->setrlimit($resource, FFI::addr($limit)) !== 0) {
                exit(1);
            }
        }
    }
}
