<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T
             * @param T $value
             * @param Closure(T): string $fn
             */
            function apply($value, $fn): void {
                assertType('T', $value);
            }
            apply(5, function (int $x): string {
                assertType('int', $x);
                return (string) $x;
            });
        