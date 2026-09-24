<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $haystack */
            function f(array $haystack): void {
                assertType('string|false', array_search(1, $haystack));
            }
        