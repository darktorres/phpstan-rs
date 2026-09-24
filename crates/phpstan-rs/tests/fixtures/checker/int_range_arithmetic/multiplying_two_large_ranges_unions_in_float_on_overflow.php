<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<9223372036854775800, 9223372036854775807> $x
             * @param int<2, 3> $y
             */
            function f(int $x, int $y): void {
                assertType('int|float', $x * $y);
            }
        