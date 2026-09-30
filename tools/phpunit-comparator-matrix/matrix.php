<?php
// Compares every pair of a fixed set of values with sebastian/comparator,
// as PHPUnit's assertEquals does, and prints the results as JSON for the
// comparator model's test (`crates/types/src/tests/comparator.rs`). The
// argument is the autoloader of an install of the comparator major wanted:
//   php tools/phpunit-comparator-matrix/matrix.php <dir>/vendor/autoload.php
// Written for PHP 8.1 and later.

error_reporting(0);
require $argv[1];

use SebastianBergmann\Comparator\ComparisonFailure;
use SebastianBergmann\Comparator\Factory;

class P { public $a = 1; }
class Q { public $a = 1; }
class S { public function __toString() { return '1'; } }
enum E { case A; case B; }

$values = [
    'null' => null,
    'true' => true,
    'false' => false,
    '0' => 0,
    '-1' => -1,
    '1' => 1,
    'PHP_INT_MAX' => PHP_INT_MAX,
    '0.0' => 0.0,
    '-0.0' => -0.0,
    '1.5' => 1.5,
    'INF' => INF,
    'NAN' => NAN,
    "''" => '',
    "'0'" => '0',
    "'1'" => '1',
    "' 1'" => ' 1',
    "'1 '" => '1 ',
    "'1.0'" => '1.0',
    "'1e1'" => '1e1',
    "'0x1A'" => '0x1A',
    "'abc'" => 'abc',
    "'ABC'" => 'ABC',
    "'1abc'" => '1abc',
    "'INF'" => 'INF',
    'array{}' => [],
    'array{1}' => [1],
    "array{'1'}" => ['1'],
    'array{1, 2}' => [1, 2],
    'array{a: 1}' => ['a' => 1],
    'array{b: 1}' => ['b' => 1],
    'P' => new P(),
    'Q' => new Q(),
    'S' => new S(),
    'E::A' => E::A,
    'E::B' => E::B,
];

$factory = new Factory();
$results = [];
foreach ($values as $a => $expected) {
    foreach ($values as $b => $actual) {
        try {
            $factory->getComparatorFor($expected, $actual)->assertEquals($expected, $actual);
            $equal = true;
        } catch (ComparisonFailure) {
            $equal = false;
        }
        $results[] = [$a, $b, $equal];
    }
}

$installed = json_decode(file_get_contents(dirname($argv[1]) . '/composer/installed.json'), true);
$version = null;
foreach ($installed['packages'] ?? $installed as $package) {
    if ($package['name'] === 'sebastian/comparator') {
        $version = $package['version'];
    }
}

echo json_encode([
    'comparator' => $version,
    'columns' => ['expected', 'actual', 'equal'],
    'results' => $results,
], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES), "\n";
