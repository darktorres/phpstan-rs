<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('array<int|false>', filter_var($x, FILTER_VALIDATE_INT, FILTER_FORCE_ARRAY));
            }
        