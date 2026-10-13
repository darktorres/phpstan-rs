<?php
// Regenerates the builtin-runs table one group of extensions at a time. Each
// group is recorded by workers that load the base modules and that group's
// modules only, so each child a worker forks pays for those alone. The rows of
// every group make one table, and its sidecar lists each group's run.
//
//   php tools/builtin-runs/regenerate.php --list
//   php tools/builtin-runs/regenerate.php --out=FILE [--skip=..] [--sandbox=..]
//       [--jobs=..] [--cache=DIR | --no-cache] [--verify] [--zend-mm]
//
// FILE must not exist: the sidecar lists every run that wrote it. The first
// group writes FILE and the others update it. Each group runs to completion
// before the next starts; the driver stops at the first group that fails.

require_once __DIR__ . '/lib/groups.php';

/** The recorder flags that every group's run takes, with a value. */
const VALUED_FLAGS = ['--skip', '--sandbox', '--jobs', '--cache'];
/** The recorder switches that every group's run takes. */
const SWITCHES = ['--no-cache', '--verify', '--zend-mm'];

$groups = recording_groups(
    get_loaded_extensions(),
    compiled_extensions(PHP_BINARY),
    array_keys(php_configuration(PHP_BINARY)['modules']),
);

if (in_array('--list', array_slice($argv, 1), true)) {
    foreach ($groups as $number => $group) {
        printf("%2d  extensions %s\n    modules    %s\n", $number + 1, implode(',', $group['extensions']), implode(',', $group['modules']) ?: '(none)');
    }
    exit(0);
}

$out = null;
$passed = [];
foreach (array_slice($argv, 1) as $argument) {
    if (str_starts_with($argument, '--out=')) {
        $out = substr($argument, strlen('--out='));
    } elseif (in_array($argument, SWITCHES, true) || (str_contains($argument, '=') && in_array(explode('=', $argument, 2)[0], VALUED_FLAGS, true))) {
        $passed[] = $argument;
    } else {
        fwrite(STDERR, "regenerate.php takes --list, or --out=FILE and the recorder's --skip, --sandbox, --jobs, --cache, --no-cache, --verify and --zend-mm\n");
        exit(2);
    }
}
if ($out === null || $out === '') {
    fwrite(STDERR, "regenerate.php needs --out=FILE\n");
    exit(2);
}
if (file_exists($out)) {
    fwrite(STDERR, "$out exists; remove it first, the sidecar lists every run that wrote it\n");
    exit(2);
}

foreach ($groups as $number => $group) {
    $command = [
        PHP_BINARY,
        __DIR__ . '/record.php',
        '--ext=' . implode(',', $group['extensions']),
        '--modules=' . implode(',', $group['modules']),
        ...$passed,
        ($number === 0 ? '--out=' : '--update=') . $out,
    ];
    fwrite(STDERR, sprintf("group %d of %d: %s\n", $number + 1, count($groups), implode(', ', $group['extensions'])));
    passthru(implode(' ', array_map('escapeshellarg', $command)), $status);
    if ($status !== 0) {
        fwrite(STDERR, "group " . ($number + 1) . " failed with status $status; the table at $out holds the groups before it\n");
        exit(1);
    }
}
fwrite(STDERR, "table and sidecar written: $out\n");
