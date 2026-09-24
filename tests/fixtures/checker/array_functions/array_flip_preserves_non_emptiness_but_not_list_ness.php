<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('non-empty-array<string, int>', array_flip($xs));
            }
        