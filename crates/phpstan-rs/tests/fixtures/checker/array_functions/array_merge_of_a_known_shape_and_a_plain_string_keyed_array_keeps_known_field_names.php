<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a: int} $defaults
             *  @param array<string, string> $options
             */
            function f(array $defaults, array $options): void {
                assertType('array{a: int|string, ...<string, string>}', array_merge($defaults, $options));
            }
        