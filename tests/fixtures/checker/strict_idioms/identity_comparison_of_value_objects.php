<?php
final class Money {
    public function equals(Money $other): bool { return $this === $other || true; }
}
final class Plain {}
enum Suit { case Hearts; public function equals(Suit $o): bool { return true; } }
function f(Money $a, Money $b, ?Money $c, Plain $p, Plain $q, Suit $s): void {
    var_dump($a === $b);
    var_dump($b !== $a);
    var_dump($c === null);
    var_dump($p === $q);
    var_dump($s === Suit::Hearts);
    var_dump($a === $p);
}
