<?php
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            /**
             * @return value-of<Suit>
             */
            function bad(): string {
                return 'not a real case';
            }
        