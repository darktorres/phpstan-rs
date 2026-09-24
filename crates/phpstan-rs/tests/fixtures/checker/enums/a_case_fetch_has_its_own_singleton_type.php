<?php
            use function PHPStan\Testing\assertType;
            enum Suit {
                case Hearts;
                case Spades;
            }
            function f(): void {
                assertType('Suit::Hearts', Suit::Hearts);
            }
        