<?php
            use function PHPStan\Testing\assertType;
            function f(string $x, int $filter): void {
                assertType('mixed', filter_var($x, $filter));
            }
        