<?php
use function PHPStan\Testing\assertType;
enum Suit: string { case H = 'h'; }
/** @template T of \UnitEnum */
class Coercer {
    /** @return value-of<T> */
    public function coerce(mixed $value): string|int { return $value; }
    /** @return T */
    public function t(): \UnitEnum { return Suit::H; }
}
/** @param Coercer<Suit> $s */
function f(Coercer $c, Coercer $s): void {
    assertType('int|string', $c->coerce('x'));
    assertType('UnitEnum', $c->t());
    assertType("'h'", $s->coerce('x'));
    assertType('Suit', $s->t());
}
