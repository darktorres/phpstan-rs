<?php
use function PHPStan\Testing\assertType;

function f(bool $c): void {
    $x = 1;
    if ($c) {
        $x = 'a';
        goto end;
    }
    $x = 2.5;
    end:
    assertType("2.5|'a'", $x);
}

function g(bool $c): void {
    $x = 1;
    if ($c) {
        $x = 'a';
        goto end;
    }
    return;
    end:
    assertType("'a'", $x);
}
