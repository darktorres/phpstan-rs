<?php
            use function PHPStan\Testing\assertType;
            function f(string $x, array $dynamic): void {
                assertType(
                    'mixed',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => $dynamic])
                );
            }
        