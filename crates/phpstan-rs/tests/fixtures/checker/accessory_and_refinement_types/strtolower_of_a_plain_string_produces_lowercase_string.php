<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('lowercase-string', strtolower($x));
            }
        