<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a: int} $shape
             *  @param array<int|string, string> $options
             */
            function f(array $shape, array $options): void {
                assertType('non-empty-array<int|string>', array_merge($shape, $options));
            }
        