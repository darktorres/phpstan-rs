<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<1, 5> $x
             * @param int<2, 4> $y
             */
            function f(int $x, int $y): void {
                assertType('int<0, 3>', $x % $y);
            }
        