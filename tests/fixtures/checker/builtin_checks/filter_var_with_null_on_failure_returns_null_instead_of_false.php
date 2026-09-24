<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'non-falsy-string|null',
                    filter_var($x, FILTER_VALIDATE_IP, FILTER_NULL_ON_FAILURE)
                );
            }
        