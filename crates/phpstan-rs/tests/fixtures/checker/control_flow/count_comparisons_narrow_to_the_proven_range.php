<?php
use function PHPStan\Testing\assertType;

/**
 * @param array{}|array{'a', 'b'} $x
 * @param int<0, max> $n
 * @param list<int> $l
 * @param array{'a', 'b'}|array{'a', 'b', 'c'} $y
 */
function f(array $x, int $n, array $l, string $csv, array $y): void {
    if (count($x) > 0) {
        assertType("array{0: 'a', 1: 'b'}", $x);
    }
    if (count($l) > $n) {
        assertType('non-empty-list<int>', $l);
    }
    $parts = explode(',', $csv);
    if (count($parts) === 2) {
        [$a, $b] = $parts;
        assertType('string', $b);
    }
    if (count($x) === 0) {
        assertType('array{}', $x);
    }
    if (count($y) < 3) {
        assertType("array{0: 'a', 1: 'b'}", $y);
    }
    $three = [1, 2, 3];
    for ($i = 0; $i < count($three); $i++) {
        assertType('int<0, 2>', $i);
    }
    if (sizeof($l) >= 2) {
        assertType('non-empty-list<int>&hasOffset(1)', $l);
    }
    if (count($l, COUNT_RECURSIVE) > 3) {
        assertType('non-empty-list<int>', $l);
    }
}

f([], 0, [1, 2], 'a,b', ['a', 'b']);
f(['a', 'b'], 1, [], 'a', ['a', 'b', 'c']);
