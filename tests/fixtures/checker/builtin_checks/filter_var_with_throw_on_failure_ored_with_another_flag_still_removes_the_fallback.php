<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int',
                    filter_var($x, FILTER_VALIDATE_INT, FILTER_FLAG_ALLOW_OCTAL | FILTER_THROW_ON_FAILURE)
                );
            }
        