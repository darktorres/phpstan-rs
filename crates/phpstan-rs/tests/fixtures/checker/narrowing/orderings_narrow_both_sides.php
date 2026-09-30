<?php
use function PHPStan\Testing\assertType;

/**
 * @param int<0, 10> $n
 * @param array<mixed>|int<min, 0> $v
 */
function f(int $i, int $n, ?int $x, $v, int $a, int $b): void {
    if ($i < $n) {
        assertType('int<min, 9>', $i);
        assertType('int<0, 10>', $n);
    }
    if ($i < 2.5) {
        assertType('int<min, 2>', $i);
    }
    if ($x < 5) {
        assertType('int<min, 4>|null', $x);
    } else {
        assertType('int<5, max>', $x);
    }
    if ($v > 0) {
        assertType('array<(int|string), mixed>', $v);
    }
    if (($a <=> $b) < 0) {
        assertType('int', $a);
    }
}

f(1, 5, null, [], 1, 2);
f(7, 3, 9, 0, 2, 1);
