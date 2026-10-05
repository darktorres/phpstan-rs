<?php
// Reads the commands `cc-record` recorded, one file each: the working
// directory, the compiler, then each argument, each ended by a NUL. Prints
// them as a compilation database, a command that differs from another only
// in the object it writes listed once, or, with --include-dirs, the
// directories they search for headers, absolute, one per line.

$includeDirs = $argv[1] === '--include-dirs';
$records = $argv[$includeDirs ? 2 : 1];

$commands = [];
foreach (glob("$records/*") as $path) {
    $fields = explode("\0", file_get_contents($path));
    array_pop($fields);
    $directory = array_shift($fields);
    $file = null;
    foreach ($fields as $i => $field) {
        if ($i > 0 && $field !== '' && $field[0] !== '-'
            && preg_match('/\.(c|cc|cpp|cxx)$/', $field)
            && $fields[$i - 1] !== '-o') {
            $file = $field;
        }
    }
    if ($file === null) {
        continue;
    }
    $commands[] = ['directory' => $directory, 'arguments' => $fields, 'file' => $file];
}
usort($commands, function ($a, $b) {
    return strcmp($a['directory'] . '/' . $a['file'], $b['directory'] . '/' . $b['file'])
        ?: strcmp(implode("\0", $a['arguments']), implode("\0", $b['arguments']));
});

$seen = [];
$unique = [];
foreach ($commands as $command) {
    $key = [$command['directory']];
    $arguments = $command['arguments'];
    for ($i = 0; $i < count($arguments); $i++) {
        if ($arguments[$i] === '-o') {
            $i++;
            continue;
        }
        $key[] = $arguments[$i];
    }
    $key = implode("\0", $key);
    if (!isset($seen[$key])) {
        $seen[$key] = true;
        $unique[] = $command;
    }
}
$commands = $unique;

if (!$includeDirs) {
    echo json_encode($commands, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES), "\n";
    exit;
}

$dirs = [];
foreach ($commands as $command) {
    $arguments = $command['arguments'];
    foreach ($arguments as $i => $argument) {
        $dir = null;
        foreach (['-I', '-isystem', '-iquote', '-idirafter'] as $option) {
            if ($argument === $option && isset($arguments[$i + 1])) {
                $dir = $arguments[$i + 1];
            } elseif (strncmp($argument, $option, strlen($option)) === 0 && strlen($argument) > strlen($option)) {
                $dir = substr($argument, strlen($option));
            }
        }
        if ($dir === null) {
            continue;
        }
        if ($dir[0] !== '/') {
            $dir = $command['directory'] . '/' . $dir;
        }
        $real = realpath($dir);
        if ($real !== false) {
            $dirs[$real] = true;
        }
    }
}
ksort($dirs);
foreach (array_keys($dirs) as $dir) {
    echo $dir, "\n";
}
