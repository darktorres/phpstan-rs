<?php
use function PHPStan\Testing\assertType;

function f(int $x, ?int $y): void {
    if ($x == 'abc') {
        assertType('0', $x);
    }
    if ($y < 5) {
        assertType('int<min, 4>|null', $y);
    } else {
        assertType('int<5, max>', $y);
    }
}

f(0, null);
f(3, 7);
