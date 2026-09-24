<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, list<int>> $byKey */
            function f(array $byKey): void {
                assertType('array<int, int>', array_intersect(...array_values($byKey)));
                assertType('array<int, int>', array_diff(...array_values($byKey)));
            }
        