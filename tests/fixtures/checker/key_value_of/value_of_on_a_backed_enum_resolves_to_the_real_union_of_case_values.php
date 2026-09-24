<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            /** @param value-of<Suit> $x */
            function f($x): void {
                assertType("'H'|'S'", $x);
            }
        