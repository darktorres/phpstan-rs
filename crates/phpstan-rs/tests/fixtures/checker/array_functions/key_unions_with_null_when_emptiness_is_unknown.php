<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $xs */
            function f(array $xs): void {
                assertType('non-decimal-int-string|null', key($xs));
            }
        