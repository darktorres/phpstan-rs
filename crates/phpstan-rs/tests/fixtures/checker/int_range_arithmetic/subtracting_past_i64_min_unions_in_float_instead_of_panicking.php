<?php
            use function PHPStan\Testing\assertType;
            /** @param int<-9223372036854775808, 0> $x */
            function f(int $x): void {
                assertType('int<min, -1>|float', $x - 1);
            }
        