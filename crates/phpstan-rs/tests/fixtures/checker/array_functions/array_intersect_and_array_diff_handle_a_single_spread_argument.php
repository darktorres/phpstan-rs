<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, list<int>> $byKey */
            function f(array $byKey): void {
                assertType('array<int<0, max>, int>', array_intersect(...array_values($byKey)));
                assertType('array<int<0, max>, int>', array_diff(...array_values($byKey)));
            }
        