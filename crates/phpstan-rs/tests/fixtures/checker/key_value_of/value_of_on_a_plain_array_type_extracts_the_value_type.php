<?php
            use function PHPStan\Testing\assertType;
            /** @param value-of<array<string, int>> $x */
            function f($x): void {
                assertType('int', $x);
            }
        