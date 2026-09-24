<?php
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            /**
             * @template T of value-of<Suit>
             */
            class Collection {}
            /** @param Collection<'H'> $c */
            function f($c) {}
        