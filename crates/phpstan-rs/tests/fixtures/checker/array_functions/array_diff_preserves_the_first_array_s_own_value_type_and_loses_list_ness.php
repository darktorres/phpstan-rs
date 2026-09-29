<?php
            use function PHPStan\Testing\assertType;
            /** @param list<int> $xs */
            function f(array $xs, array $other): void {
                assertType('array<int<0, max>, int>', array_diff($xs, $other));
            }
        