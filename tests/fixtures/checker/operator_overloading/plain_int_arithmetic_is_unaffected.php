<?php
            use function PHPStan\Testing\assertType;
            function f(int $a, int $b): void {
                assertType('int', $a + $b);
                assertType('int|float', $a / $b);
            }
        