<?php
            use function PHPStan\Testing\assertType;
            function f(int $x, string $y): void {
                assertType('array{x: int, y: string}', get_defined_vars());
            }
        