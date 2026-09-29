<?php
use function PHPStan\Testing\assertType;
function f(int|float|string $x): void {
    if (is_float($x)) {
        assertType('float', $x);
    }
}
