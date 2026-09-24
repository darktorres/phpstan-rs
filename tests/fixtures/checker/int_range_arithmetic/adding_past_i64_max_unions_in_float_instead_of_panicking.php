<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, 9223372036854775807> $x */
            function f(int $x): void {
                assertType('int<1, max>|float', $x + 1);
            }
        