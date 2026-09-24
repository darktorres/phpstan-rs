<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('mixed', filter_var($x, FILTER_VALIDATE_INT, FILTER_REQUIRE_ARRAY));
            }
        