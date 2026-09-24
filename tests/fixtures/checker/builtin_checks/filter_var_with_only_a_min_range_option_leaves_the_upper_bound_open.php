<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int<1, max>|false',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]])
                );
            }
        