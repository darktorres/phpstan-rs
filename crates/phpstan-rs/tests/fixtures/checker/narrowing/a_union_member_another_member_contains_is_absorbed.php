<?php
use function PHPStan\Testing\assertType;
enum Suit { case Hearts; case Clubs; }
interface Shape {}
/**
 * @param Suit::Hearts|Suit $s
 * @param object|Shape $o
 * @param non-empty-string|string $t
 */
function f($s, $o, $t): void {
    assertType('Suit', $s);
    assertType('object', $o);
    assertType('string', $t);
}
