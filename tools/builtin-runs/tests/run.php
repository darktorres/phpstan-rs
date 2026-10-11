<?php
// The recorder's own tests: the table and what is kept beside it, the cache
// key, the selection and the stub index. Run with `php tests/run.php`; exits 1
// when a check fails.

declare(strict_types=0);

error_reporting(E_ALL);
ini_set('display_errors', '1');

require_once dirname(__DIR__) . '/lib/seen.php';
require_once dirname(__DIR__) . '/lib/probes.php';
require_once dirname(__DIR__) . '/lib/child.php';
require_once dirname(__DIR__) . '/lib/plan.php';
require_once dirname(__DIR__) . '/lib/unit.php';
require_once dirname(__DIR__) . '/lib/protocol.php';
require_once dirname(__DIR__) . '/lib/workdir.php';
require_once dirname(__DIR__) . '/lib/pool.php';
require_once dirname(__DIR__) . '/lib/stats.php';
require_once dirname(__DIR__) . '/lib/select.php';
require_once dirname(__DIR__) . '/lib/table.php';
require_once dirname(__DIR__) . '/lib/cache.php';
require_once dirname(__DIR__) . '/lib/sidecar.php';
require_once dirname(__DIR__) . '/lib/coverage.php';

$failures = [];
$checks = 0;

/** Records a check. */
function check(string $name, bool $holds, string $detail = ''): void
{
    global $failures, $checks;
    $checks++;
    if (!$holds) {
        $failures[] = $name . ($detail === '' ? '' : ": $detail");
    }
}

$scratch = sys_get_temp_dir() . '/builtin-runs-tests-' . getmypid();
@mkdir($scratch);

// The table: sorted as bytes, one row to a line, with the trailing newline.
check('table_text sorts as bytes', table_text(["b\t\tprobed\t", "a\t\tprobed\t", "A\t\tprobed\t"]) === "A\t\tprobed\t\na\t\tprobed\t\nb\t\tprobed\t\n");
check('table_text of no rows is one newline', table_text([]) === "\n");

// Merging a partial run: the selected builtins' rows go, the others stay.
$selected = static fn (string $builtin): bool => $builtin === 'ds\\deque::push';
$existing = ["ds\\deque::push\t\tprobed\t", "ds\\deque::push\t0\tcalls\t", "ds\\map::put\t\tprobed\t", "decimal\\decimal::__tostring\t\tprobed\t"];
$merged = merge_rows($existing, ["ds\\deque::push\t\tprobed\t"], $selected);
sort($merged, SORT_STRING);
check('merge removes the selected builtin and keeps the rest', $merged === ["decimal\\decimal::__tostring\t\tprobed\t", "ds\\deque::push\t\tprobed\t", "ds\\map::put\t\tprobed\t"], (string) json_encode($merged));
check('merge keeps a row of a builtin named in a by: place', merge_rows(["x::y\tby:z:0\tcalls\t"], [], static fn (string $b): bool => $b === 'x::z') === ["x::y\tby:z:0\tcalls\t"]);

// A table written atomically: the content is there, and no temporary file is left.
$path = "$scratch/table.tsv";
write_atomic($path, "one\n");
check('write_atomic writes the content', file_get_contents($path) === "one\n");
write_atomic($path, "two\n");
check('write_atomic replaces the old file', file_get_contents($path) === "two\n");
check('write_atomic leaves no temporary file', count(glob("$scratch/*.tmp.*") ?: []) === 0);
// A write that cannot complete leaves the old file.
$child = escapeshellarg(PHP_BINARY) . ' -r ' . escapeshellarg(
    'require ' . var_export(dirname(__DIR__) . '/lib/table.php', true) . '; write_atomic(' . var_export("$scratch/no-such-dir/x.tsv", true) . ', "x");'
);
exec($child . ' 2>/dev/null', $output, $status);
check('write_atomic exits 1 when the directory is missing', $status === 1, "status $status");
check('the failed write left the old file', file_get_contents($path) === "two\n");

// The committed table is in the grammar build.rs reads: four tab-separated
// fields, the builtin and the detail lowercased, the place and the kind known.
$committed = dirname(__DIR__, 3) . '/crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv';
$bad = [];
$kinds = [];
foreach (file($committed, FILE_IGNORE_NEW_LINES) as $line) {
    $fields = explode("\t", $line);
    if (count($fields) !== 4) {
        $bad[] = "not four fields: $line";
        continue;
    }
    [$builtin, $place, $kind, $detail] = $fields;
    $kinds[$kind] = true;
    if ($builtin === '' || $builtin !== strtolower($builtin) || $detail !== strtolower($detail)) {
        $bad[] = "not lowercased: $line";
    }
    if (!preg_match('/^(new:|by:[a-z0-9_]+:)?\d+(\[\])?$|^$/', $place)) {
        $bad[] = "place: $line";
    }
    if (!in_array($kind, ['probed', 'calls', 'method', 'wrapper', 'releases'], true)) {
        $bad[] = "kind: $line";
    }
}
check('committed table rows follow the grammar', $bad === [], implode('; ', array_slice($bad, 0, 3)));
check('committed table has the five kinds', count($kinds) === 5, implode(',', array_keys($kinds)));

// Counts of rows by kind, and of the `by:` rows.
$counts = row_counts(["a\t\tprobed\t", "a\tby:m:0\tcalls\t", "a\t0\tcalls\t"]);
check('row_counts counts kinds and by: rows', $counts === ['by' => 1, 'calls' => 2, 'probed' => 1], (string) json_encode($counts));

// Two runs of a unit: `probed` and `releases` only where both agree.
[$rows, $differs] = reconcile(
    ["a\t\tprobed\t" => true, "a\t0\tcalls\t" => true, "a\t0\treleases\t" => true],
    ["a\t\tprobed\t" => true, "a\t1\tcalls\t" => true],
);
sort($rows, SORT_STRING);
check('reconcile keeps probed when both found it', in_array("a\t\tprobed\t", $rows, true));
[$alone] = reconcile(["a\t\tprobed\t" => true], []);
check('reconcile drops probed when one run found it', $alone === []);
check('reconcile keeps calls seen in either run', in_array("a\t0\tcalls\t", $rows, true) && in_array("a\t1\tcalls\t", $rows, true));
check('reconcile drops releases seen in one run', !in_array("a\t0\treleases\t", $rows, true));
check('reconcile reports that the runs differed', $differs);
[, $same] = reconcile(["a\t\tprobed\t" => true], ["a\t\tprobed\t" => true]);
check('reconcile reports equal runs as equal', !$same);

// The cache key changes with each thing that can change a result, and not with the order of the unit's parts.
$unit = ['method', 'Ds\\Deque', 'push', 0, 0];
$base = ['recorder' => 'r', 'binary' => '1/2', 'manifest' => 'm', 'unit' => $unit, 'signature' => 's', 'skip' => 'off', 'sandbox' => 'none', 'known' => null];
$key = cache_key($base);
check('cache key is stable', $key === cache_key($base));
foreach (['recorder' => 'r2', 'binary' => '1/3', 'manifest' => 'm2', 'signature' => 's2', 'skip' => 'dead', 'sandbox' => 'bwrap', 'known' => ['ds\\deque::push' => 1]] as $field => $value) {
    check("cache key changes with $field", cache_key([$field => $value] + $base) !== $key);
}
check('cache key changes with the unit', cache_key(['unit' => ['method', 'Ds\\Deque', 'pop', 0, 0]] + $base) !== $key);

// The manifest: Core and FFI have no info hash (their text differs between runs),
// the other extensions do, and each lists what it provides.
$manifest = manifest();
check('Core has no info hash', array_key_exists('Core', $manifest['extensions']) && $manifest['extensions']['Core']['info'] === null);
check('a stable extension has an info hash', is_string($manifest['extensions']['standard']['info'] ?? null));
check('an extension lists what it provides', in_array('strlen', $manifest['extensions']['Core']['provides'], true) || in_array('strlen', $manifest['extensions']['standard']['provides'], true));
check('manifest is equal on a second call', manifest() === $manifest);

// The selection: by regex, by extension, or both.
$byRegex = selector('#^decimal\\\\#', null);
check('a regex selects by lowercased name', $byRegex('decimal\\decimal::__tostring') && !$byRegex('ds\\deque::push'));
$byExtension = selector(null, ['decimal']);
check('an extension selects its functions and methods', $byExtension('decimal\\decimal::__tostring') && !$byExtension('ds\\deque::push'));
$both = selector('#^decimal\\\\decimal::#', ['ds']);
check('a regex and an extension select the intersection', !$both('decimal\\decimal::__tostring') && !$both('ds\\deque::push'));
check('no selection selects everything', selector(null, null)('anything::at_all'));

// The stub index: its declarations are read, with the extension they belong to.
$stubs = stub_declarations();
check('the stub index has functions and classes', count($stubs) > 1000 && in_array('class', array_column($stubs, 0), true));

// The sidecar: the table's hash, and each run that wrote it.
$sidecar = json_decode(sidecar_text(["a\t\tprobed\t"], $manifest, 'rec', 'off', ['selection' => [], 'flags' => [], 'totals' => []], []), true);
check('the sidecar hashes the table it describes', $sidecar['table']['sha256'] === hash('sha256', "a\t\tprobed\t\n"));
check('the sidecar records one run', count($sidecar['runs']) === 1);
check('the sidecar path drops .tsv', sidecar_path('/x/builtin-runs-8.5.tsv') === '/x/builtin-runs-8.5.meta.json');

@array_map('unlink', glob("$scratch/*") ?: []);
@rmdir($scratch);

if ($failures !== []) {
    foreach ($failures as $failure) {
        fwrite(STDERR, "FAIL $failure\n");
    }
    fwrite(STDERR, count($failures) . " of $checks checks failed\n");
    exit(1);
}
echo "$checks checks passed\n";
