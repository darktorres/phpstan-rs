<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<9223372036854775800, 9223372036854775807> $x
             */
            function f(int $x): void {
                assertType('int<9223372036854775801, max>|float', $x + 1);
            }
        