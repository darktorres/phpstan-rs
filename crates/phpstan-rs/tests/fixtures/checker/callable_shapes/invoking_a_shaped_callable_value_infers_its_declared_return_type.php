<?php
            use function PHPStan\Testing\assertType;
            /** @param Closure(int): string $fn */
            function test($fn): void {
                assertType('string', $fn(1));
            }
        