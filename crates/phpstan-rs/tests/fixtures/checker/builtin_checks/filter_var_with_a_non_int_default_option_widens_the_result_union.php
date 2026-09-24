<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int|null',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['default' => null]])
                );
            }
        