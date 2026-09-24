<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, 10> $x */
            function f(int $x): void {
                assertType('int<-1, 9>', $x - 1);
            }
        