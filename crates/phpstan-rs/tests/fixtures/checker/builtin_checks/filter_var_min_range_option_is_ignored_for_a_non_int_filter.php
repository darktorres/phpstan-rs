<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'float|false',
                    filter_var($x, FILTER_VALIDATE_FLOAT, ['options' => ['min_range' => 1]])
                );
            }
        