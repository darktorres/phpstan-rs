<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string> $values */
            function f(array $values): void {
                assertType('array<(int|string), int|false>', filter_var($values, FILTER_VALIDATE_INT, FILTER_REQUIRE_ARRAY));
            }
        