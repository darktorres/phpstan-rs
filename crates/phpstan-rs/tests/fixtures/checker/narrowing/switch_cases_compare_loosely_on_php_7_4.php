<?php
use function PHPStan\Testing\assertType;

/**
 * @param int|string $x
 */
function f($x): void {
    switch ($x) {
        case 0:
            assertType('0|string', $x);
            break;
    }
}

f('abc');
f(0);
f(3);
