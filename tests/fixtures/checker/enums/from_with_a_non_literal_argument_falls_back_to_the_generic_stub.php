<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            function f(string $x): void {
                assertType('Suit', Suit::from($x));
            }
        