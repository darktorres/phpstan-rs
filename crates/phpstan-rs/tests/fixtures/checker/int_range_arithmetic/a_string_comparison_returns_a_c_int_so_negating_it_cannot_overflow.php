<?php
use function PHPStan\Testing\assertType;

function f(string $a, string $b): void {
    assertType('int<-2147483648, 2147483647>', strcmp($a, $b));
    assertType('int<-2147483647, 2147483648>', -strcmp($a, $b));
    assertType('int<-2147483648, 2147483647>', strnatcasecmp($a, $b));
    assertType('int<-2147483648, 2147483647>', substr_compare($a, $b, 0));
    assertType('int<-2147483648, 2147483647>', strcoll($a, $b));
}

f('a', 'zzz');
f('zzz', 'a');
