<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, max> $x */
            function f(int $x): void {
                assertType('0', $x * 0);
            }
        