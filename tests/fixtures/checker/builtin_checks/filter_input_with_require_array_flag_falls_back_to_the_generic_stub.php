<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType(
                    'mixed',
                    filter_input(INPUT_GET, 'x', FILTER_VALIDATE_INT, FILTER_REQUIRE_ARRAY)
                );
            }
        