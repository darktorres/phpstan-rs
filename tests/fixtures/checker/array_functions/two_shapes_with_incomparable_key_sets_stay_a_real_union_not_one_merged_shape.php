<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array{a: int} $shapeA
             * @param array{b: string} $shapeB
             */
            function f(array $shapeA, array $shapeB, bool $cond): void {
                $x = $cond ? $shapeA : $shapeB;
                assertType('array{a: int}|array{b: string}', $x);
            }
        