<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<1, 5> $x
             * @param int<1, 5> $y
             */
            function f(int $x, int $y): void {
                assertType('int<2, 10>', $x + $y);
            }
        