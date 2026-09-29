<?php
use function PHPStan\Testing\assertType;
enum Suit { case Hearts; case Clubs; case Spades; }
function f(Suit $x, ?Suit $y): void {
    if ($x === Suit::Hearts) { return; }
    assertType('Suit::Clubs|Suit::Spades', $x);
    if ($x !== Suit::Clubs) {
        assertType('Suit::Spades', $x);
    }
    if ($y === Suit::Hearts || $y === null) { return; }
    assertType('Suit::Clubs|Suit::Spades', $y);
}
function g(Suit $x): int {
    return match ($x) {
        Suit::Hearts => 1,
        default => (function () use ($x) { assertType('Suit::Clubs|Suit::Spades', $x); return 2; })(),
    };
}
