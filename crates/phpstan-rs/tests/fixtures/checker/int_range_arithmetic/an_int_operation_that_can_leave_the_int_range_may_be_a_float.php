<?php
use function PHPStan\Testing\assertType;

/**
 * @param int<0, max> $a
 * @param int<0, max> $b
 */
function f(int $n, int $a, int $b): void {
    assertType('int<-9223372036854775807, max>|float', $n + 1);
    assertType('int|float', $n * 2);
    assertType('int<min, 9223372036854775806>|float', $n - 1);
    assertType('int<-9223372036854775807, max>', $a - $b);
    assertType('int<-9223372036854775807, max>|float', -$n);
    assertType('int<0, max>|float', abs($n));
    $m = $n;
    $m++;
    assertType('int<-9223372036854775807, max>|float', $m);
    $r = $n * 3;
    if (is_float($r)) {
        assertType('float', $r);
        return;
    }
    assertType('int', $r);
}

f(PHP_INT_MAX, PHP_INT_MAX, 0);
f(PHP_INT_MIN, 0, PHP_INT_MAX);
