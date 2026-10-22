<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $xs */
            function f(array $xs): void {
                assertType('array<(int|string), int<0, 1073741823>>', array_flip($xs));
            }
        