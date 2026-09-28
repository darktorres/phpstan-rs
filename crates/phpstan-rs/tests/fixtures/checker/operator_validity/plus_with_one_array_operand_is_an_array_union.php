<?php
            use function PHPStan\Testing\assertType;
            function f(mixed $a): void {
                assertType('non-empty-array<(int|string), mixed>', $a + ['x' => 1]);
            }
        