<?php
use function PHPStan\Testing\assertType;

/** @return void */
function f() {
    $abc = 'abc';
    assertType("'c'|''", $abc[-1]);
    $w = 'ab';
    $w[-1] = 'x';
    assertType("'ax'|'ab'", $w);
    $v = 'ab';
    $v[-5] = 'x';
    assertType("'ab'", $v);
}

f();
