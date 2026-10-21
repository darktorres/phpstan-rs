<?php
use function PHPStan\Testing\assertType;
function f(bool $w): void {
    do {
        $n = 'x';
        assertType('bool', $w);
    } while ($w = !$w);
    assertType('false', $w);
}
