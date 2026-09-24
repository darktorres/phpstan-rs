<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a: 1, b: string} $shape */
            function f(array $shape): void {
                assertType("array{1: 'a', ...}", array_flip($shape));
            }
        