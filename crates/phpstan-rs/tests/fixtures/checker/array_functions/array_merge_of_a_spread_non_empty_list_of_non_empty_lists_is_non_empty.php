<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<non-empty-list<int>> $parts */
            function f(array $parts): void {
                assertType('non-empty-list<int>', array_merge(...$parts));
            }
        