<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('non-empty-array<string, int<0, max>>', array_flip($xs));
            }
        