<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a: 1, b: 1} $shape */
            function f(array $shape): void {
                assertType("array{1: 'b'}", array_flip($shape));
            }
        