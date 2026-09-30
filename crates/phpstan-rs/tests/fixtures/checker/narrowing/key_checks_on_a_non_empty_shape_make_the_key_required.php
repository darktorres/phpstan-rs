<?php
use function PHPStan\Testing\assertType;

/**
 * @param 'a'|'b' $k
 */
function f(string $k): void {
    $c = [];
    $c[$k] = 'x';
    if (isset($c['a'])) {
        assertType("array{a: 'x', b?: 'x'}", $c);
    }
    if (array_key_exists('a', $c)) {
        assertType("array{a: 'x', b?: 'x'}", $c);
    } else {
        assertType("array{b: 'x'}", $c);
    }
}

f('a');
f('b');
