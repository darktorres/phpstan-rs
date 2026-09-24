<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array<string, int> $a
             * @param array<string, bool> $b
             */
            function f(array $a, array $b): void {
                assertType('array<string, int>', array_intersect_key($a, $b));
            }
        