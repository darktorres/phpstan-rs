<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            function f(): void {
                assertType('Suit::Hearts', Suit::tryFrom('H'));
            }
        