<?php
use function PHPStan\Testing\assertType;
/**
 * @psalm-type A = string
 * @psalm-type S = array{
 *     stack: list<A>,
 * }
 * @phpstan-type C = array{
 *     x?: A|S|null,
 * }
 * @phpstan-type Loop1 = array{next: Loop2}
 * @phpstan-type Loop2 = array{next: Loop1}
 */
final class R {
    /**
     * @param C $c
     * @param Loop1 $l
     */
    public static function f(array $c, array $l): void {
        assertType('array{x?: string|array{stack: list<string>}|null}', $c);
        assertType('array{next: array{next: mixed}}', $l);
    }
}
