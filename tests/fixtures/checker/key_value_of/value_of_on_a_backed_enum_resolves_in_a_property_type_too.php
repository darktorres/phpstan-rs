<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            class Holder {
                /** @var value-of<Suit> */
                public $x;
            }
            function f(Holder $h): void {
                assertType("'H'|'S'", $h->x);
            }
        