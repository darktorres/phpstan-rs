<?php
            use function PHPStan\Testing\assertType;
            /** @param key-of<array{a: int, 5: string}> $x */
            function f($x): void {
                assertType("'a'|5", $x);
            }
        