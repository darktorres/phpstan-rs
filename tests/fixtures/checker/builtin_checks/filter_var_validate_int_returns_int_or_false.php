<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('int|false', filter_var($x, FILTER_VALIDATE_INT));
            }
        