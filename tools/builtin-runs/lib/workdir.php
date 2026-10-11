<?php
// The directories a run makes: one for each worker, and in it one for each
// child, so that what a probe leaves is seen by no other.

/** Removes a directory and what is in it, whatever the modes are. */
function wipe_directory(string $dir): void
{
    @chmod($dir, 0700);
    $names = @scandir($dir);
    if ($names !== false) {
        foreach ($names as $name) {
            if ($name === '.' || $name === '..') {
                continue;
            }
            $path = $dir . '/' . $name;
            if (is_link($path) || !is_dir($path)) {
                @unlink($path);
            } else {
                wipe_directory($path);
            }
        }
    }
    @rmdir($dir);
}

final class Workdir
{
    private static string $base = '';
    private static int $made = 0;

    /** The name of the directory a worker with this process id works in. */
    public static function of(int $pid): string
    {
        return sys_get_temp_dir() . '/builtin-runs-' . $pid;
    }

    /** Makes the worker's directory and works in it. */
    public static function create(): void
    {
        self::$base = self::of((int) getmypid());
        wipe_directory(self::$base);
        if (!@mkdir(self::$base) || !@chdir(self::$base)) {
            fwrite(STDERR, 'cannot make ' . self::$base . "\n");
            exit(1);
        }
    }

    public static function remove(): void
    {
        chdir('/');
        wipe_directory(self::$base);
    }

    /**
     * A new directory for one child: empty but for the file `x`, which is
     * what the generated string probe names. A call that takes a file name
     * reaches its payload only when the file is there, has content and can
     * be written, and the rows of such a call (`copy` into a wrapper,
     * `fputcsv`, `openssl_pkcs7_encrypt`) were seen only when an earlier
     * probe of the same worker left one by accident.
     */
    public static function fresh(): ?string
    {
        $dir = self::$base . '/c' . ++self::$made;
        if (!@mkdir($dir) || @file_put_contents($dir . '/x', "x\n") === false) {
            return null;
        }
        return $dir;
    }

    /** Removes the directory of a child that has been reaped. */
    public static function discard(string $dir): void
    {
        wipe_directory($dir);
    }
}

/** Removes the directories of workers that no longer run (a killed run leaves them). */
function sweep_stale_directories(): int
{
    $removed = 0;
    foreach (glob(sys_get_temp_dir() . '/builtin-runs-*', GLOB_ONLYDIR) ?: [] as $dir) {
        $pid = substr($dir, strrpos($dir, '-') + 1);
        if (ctype_digit($pid) && !is_dir('/proc/' . $pid)) {
            wipe_directory($dir);
            $removed++;
        }
    }
    return $removed;
}
