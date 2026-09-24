<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType(
                    'int|null|false',
                    filter_input(INPUT_GET, 'x', FILTER_VALIDATE_INT, ['options' => ['default' => 42]])
                );
            }
        