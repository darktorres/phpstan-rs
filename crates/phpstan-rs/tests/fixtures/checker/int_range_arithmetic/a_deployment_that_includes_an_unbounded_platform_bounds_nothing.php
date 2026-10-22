<?php
use function PHPStan\Testing\assertType;

function f(string $s): void {
    assertType('int<0, max>', strlen($s));
    assertType('int<1, max>|float', strlen($s) + 1);
    assertType('int', time());
    assertType('int<-9223372036854772208, max>|float', time() + 3600);
}

f('abc');
