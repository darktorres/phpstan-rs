<?php
            use function PHPStan\Testing\assertType;
            /** @param list<array<string, int>> $byGroup */
            function f(array $byGroup): void {
                assertType('array<string, int>', array_merge(...$byGroup));
            }
        