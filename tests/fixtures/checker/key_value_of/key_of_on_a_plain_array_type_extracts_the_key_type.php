<?php
            use function PHPStan\Testing\assertType;
            /** @param key-of<array<string, int>> $x */
            function f($x): void {
                assertType('string', $x);
            }
        