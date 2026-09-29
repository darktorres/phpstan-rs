<?php
use function PHPStan\Testing\assertType;

/**
 * @param iterable<string, int> $it
 * @param Traversable<int, bool> $t
 * @param iterable<float> $values
 */
function f(iterable $it, Traversable $t, iterable $values): void
{
    foreach ($it as $k => $v) {
        assertType('int|string', $k);
        assertType('int', $v);
    }
    foreach ($t as $k => $v) {
        assertType('int', $k);
        assertType('bool', $v);
    }
    foreach ($values as $v) {
        assertType('float', $v);
    }
}
