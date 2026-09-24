<?php
            enum Suit {
                case Hearts;
            }
            function f(UnitEnum $s): void {}
            f(Suit::Hearts);
        