<?php
// The results of units kept between runs. A key holds everything that can
// change a result, so a stored result is used only where nothing has changed.

/** The hash of the recorder's own files: a change to any of them invalidates every result. */
function recorder_hash(): string
{
    $root = dirname(__DIR__);
    $files = array_merge(["$root/record.php", "$root/worker.php"], glob(__DIR__ . '/*.php') ?: []);
    sort($files);
    $hash = hash_init('sha256');
    foreach ($files as $file) {
        hash_update($hash, basename($file) . "\0" . (string) file_get_contents($file) . "\0");
    }
    return hash_final($hash);
}

/** @param array<string, mixed> $parts */
function cache_key(array $parts): string
{
    return hash('sha256', serialize($parts));
}

/**
 * The rows a unit gave under this key, or `null` when none is stored.
 *
 * @return list<string>|null
 */
function cache_read(string $dir, string $key): ?array
{
    $text = @file_get_contents("$dir/$key.json");
    if ($text === false) {
        return null;
    }
    $rows = json_decode($text, true);
    return is_array($rows) ? array_values(array_map(strval(...), $rows)) : null;
}

/** @param list<string> $rows */
function cache_write(string $dir, string $key, array $rows): void
{
    if (!is_dir($dir) && !@mkdir($dir, 0777, true) && !is_dir($dir)) {
        fwrite(STDERR, "cannot make $dir\n");
        exit(1);
    }
    write_atomic("$dir/$key.json", (string) json_encode($rows, JSON_UNESCAPED_SLASHES | JSON_INVALID_UTF8_SUBSTITUTE));
}
