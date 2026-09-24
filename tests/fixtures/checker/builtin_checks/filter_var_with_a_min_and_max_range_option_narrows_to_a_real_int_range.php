<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int<1, 10>|false',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1, 'max_range' => 10]])
                );
            }
        