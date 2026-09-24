<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $keys */
            function f(array $keys): void {
                assertType('array<string, 42>', array_fill_keys($keys, 42));
            }
        