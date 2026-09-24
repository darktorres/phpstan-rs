<?php
            use function PHPStan\Testing\assertType;
            /** @param value-of<array{a: int, b: string}> $x */
            function f($x): void {
                assertType('int|string', $x);
            }
        