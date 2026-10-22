<?php
            use function PHPStan\Testing\assertType;
            function f(mixed $a, int $b): void {
                assertType('int|float', $a + $b);
            }
