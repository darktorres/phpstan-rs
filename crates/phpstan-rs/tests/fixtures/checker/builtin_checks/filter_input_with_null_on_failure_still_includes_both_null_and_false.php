<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType(
                    'int|null|false',
                    filter_input(INPUT_GET, 'x', FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE)
                );
            }
        