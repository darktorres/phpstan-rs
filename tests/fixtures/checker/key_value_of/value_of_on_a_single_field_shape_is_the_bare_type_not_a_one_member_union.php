<?php
            use function PHPStan\Testing\assertType;
            /** @param value-of<array{a: int}> $x */
            function f($x): void {
                assertType('int', $x);
            }
        