<?php
use function PHPStan\Testing\assertType;

function f(string $s): void {
    assertType('int<1, max>|float', strlen($s) + 1);
    assertType('int<-9223372036854772208, max>|float', time() + 3600);
}

f('abc');
