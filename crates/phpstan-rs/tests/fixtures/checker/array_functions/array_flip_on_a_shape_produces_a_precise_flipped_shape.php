<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a: 1, b: 'x'} $shape */
            function f(array $shape): void {
                assertType("array{1: 'a', x: 'b'}", array_flip($shape));
            }
        