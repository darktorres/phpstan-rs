<?php
use function PHPStan\Testing\assertType;

function f(bool $c): void {
    $x = 1;
    top:
    assertType('1|2', $x);
    $x = 2;
    if ($c) {
        goto top;
    }
    assertType('2', $x);
}

function g(int $n): void {
    $i = 0;
    again:
    $i++;
    if ($i < $n) {
        goto again;
    }
    assertType('int<1, max>', $i);
}
