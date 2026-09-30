<?php
use function PHPStan\Testing\assertType;

/**
 * @param int|string|null $x
 */
function f($x): void {
    if (in_array($x, ['1', 'a'])) {
        assertType("1|numeric-string|'a'", $x);
    }
}

f('1');
f(0);
