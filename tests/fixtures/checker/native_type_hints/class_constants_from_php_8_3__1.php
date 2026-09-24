<?php
enum Suit { case Hearts; }
class Base {
    const LIMIT = 10;
    const NAMES = ['a', 'b'];
    const int TYPED = 1;
    /** @var non-empty-string */
    const LABEL = 'x';
    const SUIT = Suit::Hearts;
    const REDECLARED = 1;
}
class Child extends Base {
    const REDECLARED = 2;
}
