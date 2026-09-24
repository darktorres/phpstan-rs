<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array{a: int, b: string} $shapeAB
             * @param array{a: int} $shapeA
             */
            function f(array $shapeAB, array $shapeA, bool $cond): void {
                $x = $cond ? $shapeAB : $shapeA;
                assertType('array{a: int, b?: string}', $x);
            }
        