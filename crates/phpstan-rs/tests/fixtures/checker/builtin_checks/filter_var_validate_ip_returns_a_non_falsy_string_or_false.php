<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('non-falsy-string|false', filter_var($x, FILTER_VALIDATE_IP));
            }
        