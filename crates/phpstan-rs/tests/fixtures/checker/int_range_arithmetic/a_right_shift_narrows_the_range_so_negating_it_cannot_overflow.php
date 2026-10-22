<?php
use function PHPStan\Testing\assertType;

/**
 * @param int<0, 70> $by
 * @param int<-100, 100> $m
 * @param int<1, 2> $two
 */
function f(int $n, int $by, int $m, int $two): void {
    assertType('int<-4611686018427387904, 4611686018427387903>', $n >> 1);
    assertType('int<-4611686018427387903, 4611686018427387904>', -($n >> 1));
    assertType('int', $n >> $by);
    assertType('int<-1, 0>', $n >> 63);
    assertType('int<-50, 50>', $m >> $two);
}

f(PHP_INT_MIN, 70, -100, 1);
f(PHP_INT_MAX, 0, 100, 2);
