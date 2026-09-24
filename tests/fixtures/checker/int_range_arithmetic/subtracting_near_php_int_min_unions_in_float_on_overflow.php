<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<-9223372036854775808, -9223372036854775800> $x
             */
            function f(int $x): void {
                assertType('int<min, -9223372036854775801>|float', $x - 1);
            }
        