<?php
            use function PHPStan\Testing\assertType;
            function f(mixed $a): void {
                assertType('array<(int|string), mixed>', $a + ['x' => 1]);
            }
        