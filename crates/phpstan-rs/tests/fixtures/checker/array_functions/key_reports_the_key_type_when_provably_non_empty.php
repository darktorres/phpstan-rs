<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-array<string, int> $xs */
            function f(array $xs): void {
                assertType('non-decimal-int-string', key($xs));
            }
        