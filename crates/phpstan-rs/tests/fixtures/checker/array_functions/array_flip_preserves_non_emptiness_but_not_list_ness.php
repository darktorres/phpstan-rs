<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('non-empty-array<(int|string), int<0, 1073741823>>', array_flip($xs));
            }
        