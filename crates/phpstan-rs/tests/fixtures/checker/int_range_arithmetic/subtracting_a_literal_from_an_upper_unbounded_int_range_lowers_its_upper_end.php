<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, max> $x */
            function f(int $x): void {
                assertType('int<-1, 9223372036854775806>', $x - 1);
            }
        