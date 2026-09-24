<?php
            enum Suit {
                case Hearts;
            }
            function f(Suit $s): void {}
            f(Suit::Hearts);
        