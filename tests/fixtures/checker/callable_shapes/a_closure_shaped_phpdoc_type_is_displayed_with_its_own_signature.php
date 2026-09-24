<?php
            use function PHPStan\Testing\assertType;
            /** @param Closure(int): string $fn */
            function f($fn): void {
                assertType('Closure(int): string', $fn);
            }
        