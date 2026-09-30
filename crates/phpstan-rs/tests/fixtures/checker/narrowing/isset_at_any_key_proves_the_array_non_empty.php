<?php
use function PHPStan\Testing\assertType;

/**
 * @param array<string, list<int>> $m
 * @param array{a?: int, b?: int} $s
 */
function f(array $m, string $k, array $s): void {
    if (isset($m[$k])) {
        assertType('non-empty-array<string, list<int>>', $m);
    }
    if (isset($s[$k])) {
        assertType('non-empty-array{a?: int, b?: int}', $s);
    }
}

f(['x' => [1]], 'x', ['a' => 1]);
f([], 'y', []);
