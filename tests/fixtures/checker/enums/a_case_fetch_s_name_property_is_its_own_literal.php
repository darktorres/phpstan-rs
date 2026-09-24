<?php
            use function PHPStan\Testing\assertType;
            enum Suit {
                case Hearts;
                case Spades;
            }
            function f(): void {
                assertType("'Hearts'", Suit::Hearts->name);
            }
        