<?php
use function PHPStan\Testing\assertType;

function steps(): void {
    $s = 'a!';
    $s++;
    assertType("'a!'", $s);
    $e = '';
    $e--;
    assertType('-1', $e);
    $b = true;
    $b++;
    assertType('true', $b);
    $n = null;
    $n--;
    assertType('null', $n);
    $m = null;
    $m++;
    assertType('1', $m);
}

steps();
