<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array<string, int> $a
             * @param array<string, int> $b
             */
            function f(array $a, array $b): void {
                assertType('array<string, int>', array_udiff($a, $b, fn($x, $y) => $x <=> $y));
            }
        