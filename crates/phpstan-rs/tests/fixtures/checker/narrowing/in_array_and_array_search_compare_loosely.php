<?php
use function PHPStan\Testing\assertType;

/**
 * @param int|string|null $x
 */
function f($x): void {
    if (in_array($x, ['1', 'a'])) {
        assertType("1|'1'|'a'|numeric-string", $x);
    }
    assertType("0|1|false", array_search($x, ['1', 'a']));
    assertType('0', array_search('1', ['1', 'a']));
}

f('1');
f(null);
