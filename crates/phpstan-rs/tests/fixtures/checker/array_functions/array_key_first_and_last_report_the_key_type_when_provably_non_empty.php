<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-array<string, int> $xs */
            function f(array $xs): void {
                assertType('string', array_key_first($xs));
                assertType('string', array_key_last($xs));
            }
        