<?php
            use function PHPStan\Testing\assertType;
            function f(string $file): void {
                assertType('array{file: string}', get_defined_vars());
                include $file;
                assertType('array<string, mixed>', get_defined_vars());
                $vars = get_defined_vars();
                echo is_array($vars['lang'] ?? null) ? 'y' : 'n';
            }
        