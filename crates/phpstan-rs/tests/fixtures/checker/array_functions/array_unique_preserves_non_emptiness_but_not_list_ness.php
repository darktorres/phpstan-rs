<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<int> $x */
            function f(array $x): void {
                assertType('non-empty-array<int, int>', array_unique($x));
            }
        