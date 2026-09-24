<?php
            use function PHPStan\Testing\assertType;
            /** @param int|string|null $x */
            function f($x): int {
                return match (true) {
                    is_int($x), is_null($x) => 1,
                    default => (function () use ($x) { assertType('string', $x); return 2; })(),
                };
            }
        