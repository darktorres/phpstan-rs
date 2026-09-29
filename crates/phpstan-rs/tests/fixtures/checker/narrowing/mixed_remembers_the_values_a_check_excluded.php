<?php
use function PHPStan\Testing\assertType;
function f(mixed $x, mixed $y, mixed $z): void {
    if (isset($x)) {
        assertType('mixed~null', $x);
        if ($x === null) { echo 1; }
    }
    if ($y !== null) {
        assertType('mixed~null', $y);
        if (is_string($y)) { assertType('string', $y); }
    }
    assertType('mixed', $y);
    if (!is_string($z)) {
        assertType('mixed~string', $z);
    }
}
