<?php
use function PHPStan\Testing\assertType;

/**
 * @param int|string $x
 * @param ?string $s
 */
function f($x, ?string $s, bool $b): void {
    if ($x == 0) {
        assertType('0|numeric-string', $x);
    }
    if ($s == null) {
        assertType("''|null", $s);
    }
    if ($b == 'abc') {
        assertType('true', $b);
    }
}

f(0, null, true);
f('0', '', false);
f('x', 'y', true);
