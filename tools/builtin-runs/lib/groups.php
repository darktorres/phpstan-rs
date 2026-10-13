<?php
// The modules a worker loads: what the PHP's ini files say, so a worker can
// load only the modules it records and nothing else. A worker that loads every
// module pays for each of them in every child it forks.

/** The modules every worker loads: the recorder runs on them. */
const BASE_MODULES = ['posix', 'ffi', 'tokenizer'];

/**
 * The modules that load only with the modules they need, or whose behaviour
 * the default configuration takes from them: a serializer module is loaded
 * beside the modules that offer it as a session or cache serializer.
 */
const MODULE_NEEDS = [
    'pdo_mysql' => ['pdo', 'mysqlnd'],
    'mysqli' => ['mysqlnd'],
    'pdo_sqlite' => ['pdo'],
    'pdo_pgsql' => ['pdo'],
    'xmlreader' => ['dom'],
    'xsl' => ['dom'],
    'redis' => ['igbinary', 'msgpack'],
    'memcached' => ['igbinary', 'msgpack'],
];

/**
 * The modules whose rows depend on other modules being loaded beside them,
 * though they load without them: PDO's statements act through a driver.
 */
const MODULE_CONTEXT = [
    'pdo' => ['pdo_sqlite', 'pdo_mysql', 'pdo_pgsql'],
];

/**
 * What the ini files of a PHP say: the file it loads, the settings of files
 * that load no module (which every worker takes), and for each module the
 * settings of the file that loads it. Each setting is `[name, value]`.
 *
 * @return array{ini: ?string, global: list<array{string, string}>, modules: array<string, list<array{string, string}>>}
 */
function php_configuration(string $php): array
{
    static $cache = [];
    if (isset($cache[$php])) {
        return $cache[$php];
    }
    $info = (string) shell_exec(escapeshellarg($php) . ' --ini 2>/dev/null');
    preg_match('/Loaded Configuration File:[ \t]+(\S.*)$/m', $info, $loaded);
    preg_match('/Scan for additional \.ini files in:[ \t]+(\S.*)$/m', $info, $scan);
    $ini = isset($loaded[1]) && $loaded[1] !== '(none)' ? trim($loaded[1], '"') : null;
    $config = ['ini' => $ini, 'global' => [], 'modules' => []];
    foreach (glob(trim($scan[1] ?? '', '"') . '/*.ini') ?: [] as $file) {
        $modules = [];
        $settings = [];
        foreach (file($file, FILE_IGNORE_NEW_LINES) ?: [] as $line) {
            $line = trim($line);
            if ($line === '' || $line[0] === ';' || $line[0] === '[') {
                continue;
            }
            if (preg_match('/^extension\s*=\s*"?([^"]+?)"?$/', $line, $match) === 1) {
                $modules[] = strtolower(basename($match[1], '.so'));
            } elseif (preg_match('/^([^=\s]+)\s*=\s*(.*)$/', $line, $match) === 1) {
                $settings[] = [$match[1], trim($match[2], '"')];
            }
        }
        if ($modules === []) {
            array_push($config['global'], ...$settings);
            continue;
        }
        foreach ($modules as $module) {
            $config['modules'][$module] = $settings;
        }
    }
    return $cache[$php] = $config;
}

/**
 * The arguments that start a worker of a PHP with the base modules and the
 * given ones, and no others: no ini file of its own, and the settings the
 * ini files give to every run and to those modules.
 *
 * @param list<string> $modules
 * @return list<string>
 */
function lean_arguments(string $php, array $modules): array
{
    $config = php_configuration($php);
    $loaded = [];
    foreach ([...BASE_MODULES, ...$modules] as $module) {
        foreach ([$module, ...(MODULE_CONTEXT[$module] ?? [])] as $member) {
            foreach ([...(MODULE_NEEDS[$member] ?? []), $member] as $needed) {
                $loaded[$needed] = true;
            }
        }
    }
    $arguments = ['-n'];
    if ($config['ini'] !== null) {
        array_push($arguments, '-c', $config['ini']);
    }
    foreach (array_keys($loaded) as $module) {
        array_push($arguments, '-d', "extension=$module");
    }
    foreach ($config['global'] as [$name, $value]) {
        array_push($arguments, '-d', "$name=$value");
    }
    foreach (array_keys($loaded) as $module) {
        foreach ($config['modules'][$module] ?? [] as [$name, $value]) {
            array_push($arguments, '-d', "$name=$value");
        }
    }
    return $arguments;
}

/**
 * The extensions a PHP has built in: what it loads with no ini file at all.
 *
 * @return list<string>
 */
function compiled_extensions(string $php): array
{
    $names = (string) shell_exec(escapeshellarg($php) . ' -n -r ' . escapeshellarg('echo implode("\n", get_loaded_extensions());'));
    return array_values(array_filter(explode("\n", $names), static fn (string $name): bool => $name !== ''));
}

/**
 * The groups a PHP records in: the built-in extensions together, and each
 * loaded module in a group of its own. Each group names the extensions it
 * records and the modules its workers load beyond the base ones. A loaded
 * extension that is neither built in nor a module of the ini files is an error.
 *
 * @param list<string> $loaded the extensions the PHP has loaded
 * @param list<string> $compiled the extensions built into the PHP, with no ini file
 * @param list<string> $configured the modules the ini files load
 * @return list<array{extensions: list<string>, modules: list<string>}>
 */
function recording_groups(array $loaded, array $compiled, array $configured): array
{
    $compiled = array_map('strtolower', $compiled);
    $configured = array_map('strtolower', $configured);
    $builtin = [];
    $groups = [];
    foreach ($loaded as $name) {
        $lower = strtolower($name);
        if (in_array($lower, $configured, true)) {
            $groups[] = ['extensions' => [$name], 'modules' => [$lower]];
        } elseif (in_array($lower, $compiled, true)) {
            $builtin[] = $name;
        } else {
            throw new RuntimeException("$name is loaded but neither built in nor loaded by the ini files");
        }
    }
    array_unshift($groups, ['extensions' => $builtin, 'modules' => []]);
    return $groups;
}
