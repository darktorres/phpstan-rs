<?php

use function PHPStan\Testing\assertType;

/**
 * @param int<0, 100> $percent
 * @param int<-5, 5> $delta
 */
function f(int $n, int $percent, int $delta): void
{
    assertType('int<1, max>', max(1, $n));
    assertType('int<min, 10>', min(10, $n));
    assertType('int<0, 100>', max($percent, $delta, 0));
    assertType('int<-5, 5>', min($percent, $delta));
    assertType('int<50, 100>', max(50, $percent));
    assertType('int<0, max>', max($n, $percent));
    assertType('7', max(3, 7));
}

f(4, 20, -2);
