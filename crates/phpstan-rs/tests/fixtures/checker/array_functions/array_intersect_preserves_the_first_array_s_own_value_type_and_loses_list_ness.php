<?php
            use function PHPStan\Testing\assertType;
            /** @param list<int> $xs */
            function f(array $xs, array $other): void {
                assertType('array<int, int>', array_intersect($xs, $other));
            }
        