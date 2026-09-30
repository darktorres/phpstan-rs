<?php
use function PHPStan\Testing\assertType;

/**
 * @param int|string $x
 */
function f($x, ?string $s): void {
    switch ($x) {
        case 0:
            assertType('0|numeric-string', $x);
            break;
    }
    switch ($s) {
        case null:
            assertType("''|null", $s);
            break;
    }
}

f('0', '');
f(1, 'a');
