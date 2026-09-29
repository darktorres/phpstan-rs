<?php
use function PHPStan\Testing\assertType;
/**
 * @param key-of<array{a: 1, ...<int, string>}> $k
 * @param value-of<array{a: 1, ...<int, string>}> $v
 * @param array{a: 1, ...<int, string>}[5] $o
 */
function f($k, $v, $o): void {
    assertType("'a'|int", $k);
    assertType("1|string", $v);
    assertType('string', $o);
}
/**
 * @template T of NoSuchBound
 * @param T $x
 */
function g($x): void {}
