<?php
use function PHPStan\Testing\assertType;

/**
 * @param array<mixed> $a
 * @param array<mixed> $b
 * @param list<string> $l
 */
function f(array $a, array $b, array $l): void {
    assertType('int<0, 1073741824>', count($a));
    assertType('int<0, 1152921504606846976>', count($a) * count($b));
    assertType('int<1, 1073741825>', count($a) + 1);
    foreach ($l as $i => $s) {
        assertType('int<0, 1073741823>', $i);
        assertType('int<1, 1073741824>', $i + 1);
    }
    assertType('int<0, 1073741824>', count($a, COUNT_NORMAL));
    assertType('int<0, max>', count($a, COUNT_RECURSIVE));
}

f([1, 2], [3], ['x', 'y']);
