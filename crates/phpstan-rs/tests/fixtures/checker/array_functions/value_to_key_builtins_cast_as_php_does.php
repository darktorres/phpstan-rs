<?php
use function PHPStan\Testing\assertType;

$filled = array_fill_keys([1.7, null], 0);
assertType("non-empty-array<'1.7'|'', 0>", $filled);

$combined = array_combine(['a', 'b'], [1, 2]);
assertType('array{a: 1, b: 2}', $combined);

try {
    array_combine(['a', 'b'], [1]);
} catch (\ValueError $e) {
}

/** @param list<array{id: int, v: string}> $rows */
function columns(array $rows): void {
    assertType('array<int, string>', array_column($rows, 'v', 'id'));
}
columns([['id' => 3, 'v' => 'x']]);

/** @param list<int|string|float> $values */
function flips(array $values): void {
    assertType('array<(int|string), int<0, max>>', array_flip($values));
    assertType('array<(int|string), int<1, max>>', array_count_values($values));
}
flips([1, 'a', 1.5]);
