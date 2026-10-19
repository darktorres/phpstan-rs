<?php

// Records what the vendored Composer's `Config` makes of a vendor directory
// setting: the data of crates/composer's vendor-dir table. Run it from the
// repository root and store the output:
//
//   php tools/composer-vendor-dir/probe.php > crates/composer/src/tests/data/vendor_dir_cases.json
//
// The base directory and the home directory are placeholders (`/ROOT`,
// `/HOME`), so the table holds no machine's paths; the Rust test puts its own
// directories in their place.

$root = dirname(__DIR__, 2);
require $root . '/vendor/composer/vendor/autoload.php';
spl_autoload_register(static function (string $class) use ($root): void {
    $prefix = 'Composer\\';
    if (strncmp($class, $prefix, strlen($prefix)) !== 0) {
        return;
    }
    $file = $root . '/vendor/composer/src/Composer/' . str_replace('\\', '/', substr($class, strlen($prefix))) . '.php';
    if (is_file($file)) {
        require $file;
    }
});

putenv('PROBE_LIB=/ROOT/lib');
// `~` expands to the user's home directory, which Composer reads from the
// account database; the output replaces that directory by `/HOME`.
$userHome = Composer\Util\Platform::getUserDirectory();

// `vendor-dir` is the manifest's setting (absent when the key is not written),
// `env` the value of COMPOSER_VENDOR_DIR (absent when it is not set).
$cases = [
    ['name' => 'default'],
    ['name' => 'relative', 'vendor-dir' => 'libs'],
    ['name' => 'nested', 'vendor-dir' => 'libs/vendor'],
    ['name' => 'dot-prefixed-with-trailing-slash', 'vendor-dir' => './libs/'],
    ['name' => 'parent', 'vendor-dir' => '../vendor'],
    ['name' => 'absolute', 'vendor-dir' => '/abs/v'],
    ['name' => 'home', 'vendor-dir' => '~/v'],
    ['name' => 'trailing-backslash', 'vendor-dir' => 'libs\\'],
    ['name' => 'drive-letter', 'vendor-dir' => 'C:/v'],
    ['name' => 'scheme', 'vendor-dir' => 'https://x/y'],
    ['name' => 'two-backslashes', 'vendor-dir' => "\\\\server\\share\\v"],
    ['name' => 'env-variable', 'vendor-dir' => '$PROBE_LIB/vendor'],
    ['name' => 'env-percent', 'vendor-dir' => '%PROBE_LIB%/vendor'],
    ['name' => 'reference-to-config', 'config' => ['lib' => 'libs'], 'vendor-dir' => '{$lib}/vendor'],
    ['name' => 'environment-absolute-over-manifest', 'vendor-dir' => 'libs', 'env' => '/abs/env'],
    ['name' => 'environment-relative-over-manifest', 'vendor-dir' => 'libs', 'env' => 'env-libs'],
    ['name' => 'environment-without-manifest', 'env' => 'env-only'],
];

$results = [];
foreach ($cases as $case) {
    if (isset($case['env'])) {
        putenv('COMPOSER_VENDOR_DIR=' . $case['env']);
    } else {
        putenv('COMPOSER_VENDOR_DIR');
    }
    $config = new Composer\Config(true, '/ROOT');
    $settings = $case['config'] ?? [];
    if (isset($case['vendor-dir'])) {
        $settings['vendor-dir'] = $case['vendor-dir'];
    }
    $config->merge(['config' => $settings], 'probe');
    try {
        $value = $config->get('vendor-dir');
    } catch (Throwable $e) {
        $value = ['exception' => get_class($e) . ': ' . $e->getMessage()];
    }
    if (is_string($value) && str_starts_with($value, $userHome . '/')) {
        $value = '/HOME' . substr($value, strlen($userHome));
    }
    $results[] = [
        'name' => $case['name'],
        'config' => $case['config'] ?? null,
        'vendor-dir' => $case['vendor-dir'] ?? null,
        'env' => $case['env'] ?? null,
        'resolved' => $value,
    ];
}

echo json_encode($results, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n";
