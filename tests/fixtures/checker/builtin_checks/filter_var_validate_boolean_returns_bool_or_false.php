<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                // `bool|false` normalizes to plain `bool` (`false`
                // is already a subtype of `bool`) - matches real
                // PHPStan too.
                assertType('bool', filter_var($x, FILTER_VALIDATE_BOOLEAN));
            }
        