<?php
// The table the recorder writes: its rows, how a partial run is merged into an
// existing table, and writes that either complete or leave the old file as it
// was.

/**
 * The bytes of a table: its rows sorted as bytes, one to a line.
 *
 * @param list<string> $rows
 */
function table_text(array $rows): string
{
    sort($rows, SORT_STRING);
    return implode("\n", $rows) . "\n";
}

/**
 * The rows of a table file, without the empty lines.
 *
 * @return list<string>
 */
function read_rows(string $path): array
{
    if (!is_file($path)) {
        return [];
    }
    return file($path, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES) ?: [];
}

/**
 * The rows of `$existing` that `$selected` does not select, by the builtin
 * named in their first field, and then `$new`.
 *
 * @param list<string> $existing
 * @param list<string> $new
 * @param Closure(string): bool $selected
 * @return list<string>
 */
function merge_rows(array $existing, array $new, Closure $selected): array
{
    $kept = array_filter($existing, static fn (string $row): bool => !$selected(explode("\t", $row)[0]));
    return array_values(array_unique(array_merge(array_values($kept), $new)));
}

/**
 * Writes a file: the content goes to a file beside it, which is renamed over
 * the old one only once it is complete. The run stops when that fails.
 */
function write_atomic(string $path, string $content): void
{
    $temporary = $path . '.tmp.' . getmypid();
    $file = @fopen($temporary, 'wb');
    if ($file === false) {
        fwrite(STDERR, "cannot write $path\n");
        exit(1);
    }
    $written = fwrite($file, $content) === strlen($content);
    if (!$written || !fflush($file) || !fclose($file) || !rename($temporary, $path)) {
        @fclose($file);
        @unlink($temporary);
        fwrite(STDERR, "cannot write $path\n");
        exit(1);
    }
}

/** Stops the run when the file system of `$path` has less than 2 GiB free: a table is never written half. */
function require_free_space(string $path): void
{
    $free = @disk_free_space(is_dir($path) ? $path : dirname($path));
    if ($free === false || $free < 2 * 1024 ** 3) {
        fwrite(STDERR, "less than 2 GiB free for $path\n");
        exit(1);
    }
}

/**
 * What two runs of one unit found, kept conservatively: a row of the kinds
 * `probed` and `releases` only when both runs found it, any other when either
 * did. Also whether the runs differed.
 *
 * @param array<string, true> $first
 * @param array<string, true> $second
 * @return array{list<string>, bool}
 */
function reconcile(array $first, array $second): array
{
    $rows = [];
    $differs = false;
    foreach (array_unique(array_merge(array_keys($first), array_keys($second))) as $row) {
        $both = isset($first[$row]) && isset($second[$row]);
        $differs = $differs || !$both;
        $kind = explode("\t", $row)[2];
        if (!in_array($kind, ['probed', 'releases'], true) || $both) {
            $rows[] = $row;
        }
    }
    return [$rows, $differs];
}

/**
 * The number of rows of each kind, and of the `by:` rows, which a row's
 * third field and first place tell apart.
 *
 * @param list<string> $rows
 * @return array<string, int>
 */
function row_counts(array $rows): array
{
    $counts = ['by' => 0];
    foreach ($rows as $row) {
        [, $place, $kind] = explode("\t", $row);
        $counts[$kind] = ($counts[$kind] ?? 0) + 1;
        if (str_starts_with($place, 'by:')) {
            $counts['by']++;
        }
    }
    ksort($counts);
    return $counts;
}
