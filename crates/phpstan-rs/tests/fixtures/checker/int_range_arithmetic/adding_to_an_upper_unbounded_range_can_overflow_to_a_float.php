<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<1, max> $x
             */
            function f(int $x): void {
                assertType('int<2, max>|float', $x + 1);
            }
        