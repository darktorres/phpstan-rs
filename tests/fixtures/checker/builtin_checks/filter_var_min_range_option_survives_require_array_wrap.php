<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string> $values */
            function f(array $values): void {
                assertType(
                    'array<(int|string), int<1, max>|false>',
                    filter_var($values, FILTER_VALIDATE_INT, [
                        'flags' => FILTER_REQUIRE_ARRAY,
                        'options' => ['min_range' => 1],
                    ])
                );
            }
        