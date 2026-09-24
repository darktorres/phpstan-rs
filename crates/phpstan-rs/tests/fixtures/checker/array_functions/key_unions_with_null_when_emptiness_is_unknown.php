<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $xs */
            function f(array $xs): void {
                assertType('string|null', key($xs));
            }
        