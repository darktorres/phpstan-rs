<?php
            use function PHPStan\Testing\assertType;
            function f(mixed $a, mixed $b): void {
                assertType('int|float|array<(int|string), mixed>', $a + $b);
            }
