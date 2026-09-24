<?php
enum Suit { case Hearts; case Spades; case Clubs; }
function f(Suit $s): string {
    $a = match ($s) { Suit::Hearts => 'h', default => 'other' };
    $b = match ($s) { Suit::Hearts => 'h', Suit::Spades => 's', Suit::Clubs => 'c' };
    switch ($s) { case Suit::Hearts: return 'h'; default: return 'x'; }
}
function g(Suit $s): string {
    if ($s === Suit::Hearts) {
        return 'h';
    } elseif ($s === Suit::Spades) {
        return 's';
    } elseif ($s === Suit::Clubs) {
        return 'c';
    }
    if ($s === Suit::Hearts) { return 'only'; }
    return '';
}
