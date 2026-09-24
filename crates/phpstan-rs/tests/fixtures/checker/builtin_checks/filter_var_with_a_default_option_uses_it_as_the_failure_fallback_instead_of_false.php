<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['default' => 42]])
                );
            }
        