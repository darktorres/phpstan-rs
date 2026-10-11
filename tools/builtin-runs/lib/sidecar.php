<?php
// The sidecar of a table: what the table was recorded with, and each run that
// wrote it, since a partial run makes it a mixture of several.

/**
 * The sidecar's text for a table.
 *
 * @param list<string> $rows the table's rows
 * @param string $sandbox the sandbox the workers ran in
 * @param list<string> $gaps the stub declarations of the recorded extensions with no row, as `extension<TAB>name`
 * @param array{php: string, extensions: array<string, array{version: ?string, info: ?string, provides: list<string>}>, ffi: bool} $manifest what the run's workers were
 * @param array<string, mixed> $run the flags, selection and totals of this run
 * @param list<array<string, mixed>> $earlier the runs that wrote the table before
 */
function sidecar_text(array $rows, array $manifest, string $recorder, string $skip, string $sandbox, array $gaps, array $run, array $earlier): string
{
    $text = table_text($rows);
    $run = $run + [
        'date' => date('c'),
        'manifest' => sha1((string) json_encode($manifest)),
        'extensions' => $manifest['extensions'],
    ];
    $data = [
        'table' => [
            'sha256' => hash('sha256', $text),
            'rows' => count($rows),
            'kinds' => row_counts($rows),
        ],
        'php' => [
            'version' => $manifest['php'],
            'ffi' => $manifest['ffi'],
        ],
        'recorder' => $recorder,
        'sandbox' => $sandbox,
        'skip' => $skip,
        'gaps' => $gaps,
        'runs' => [...$earlier, $run],
    ];
    return (string) json_encode($data, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_INVALID_UTF8_SUBSTITUTE) . "\n";
}

/** The sidecar beside a table: the table's name without `.tsv`, and `.meta.json`. */
function sidecar_path(string $table): string
{
    return preg_replace('/\.tsv$/', '', $table) . '.meta.json';
}

/**
 * The runs that wrote a table before, from its sidecar; none when there is no
 * sidecar or it cannot be read.
 *
 * @return list<array<string, mixed>>
 */
function earlier_runs(string $tablePath): array
{
    $text = @file_get_contents(sidecar_path($tablePath));
    $data = $text === false ? null : json_decode($text, true);
    return is_array($data) && is_array($data['runs'] ?? null) ? array_values($data['runs']) : [];
}
