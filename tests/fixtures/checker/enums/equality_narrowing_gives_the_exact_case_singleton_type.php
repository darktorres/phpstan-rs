<?php
            use function PHPStan\Testing\assertType;
            enum Suit {
                case Hearts;
                case Spades;
            }
            function f(Suit $s): void {
                if ($s === Suit::Hearts) {
                    assertType('Suit::Hearts', $s);
                }
            }
        