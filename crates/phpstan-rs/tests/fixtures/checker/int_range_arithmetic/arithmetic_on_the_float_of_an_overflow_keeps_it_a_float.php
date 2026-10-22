<?php

use function PHPStan\Testing\assertType;

/** @param int<0, max> $n */
function f(int $n): void
{
    $big = $n * 2;
    assertType('int<0, max>|float', $big);
    assertType('int<1, max>|float', $big + 1);
    assertType('int<-1, 9223372036854775806>|float', $big - 1);
    assertType('int<0, max>|float', $big * 3);
    assertType('int<-2, 2>', $big % 3);
}

f(4);
