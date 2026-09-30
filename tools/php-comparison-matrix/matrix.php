<?php
// Evaluates PHP's comparison operators over every pair of a fixed set of
// values and prints the results as JSON, for the comparison model's
// test (`crates/types/src/tests/comparison.rs`). Run it with the PHP
// whose semantics are wanted:
//   php tools/php-comparison-matrix/matrix.php > crates/types/src/tests/data/comparison-8.5.json
// Written for PHP 7.4 and later.

error_reporting(0);

class P { public $a = 1; }
class Q { public $a = 1; }
class S { public function __toString() { return '1'; } }

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
];

$results = [];
foreach ($values as $a => $x) {
    foreach ($values as $b => $y) {
        $results[] = [
            $a,
            $b,
            $x == $y,
            $x < $y,
            $x <= $y,
            $x > $y,
            $x >= $y,
            $x <=> $y,
        ];
    }
}

echo json_encode([
    'php' => PHP_MAJOR_VERSION . '.' . PHP_MINOR_VERSION,
    'columns' => ['a', 'b', '==', '<', '<=', '>', '>=', '<=>'],
    'results' => $results,
], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES), "\n";
