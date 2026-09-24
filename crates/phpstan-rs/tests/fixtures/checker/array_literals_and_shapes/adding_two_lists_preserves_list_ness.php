<?php
            use function PHPStan\Testing\assertType;
            /** @param list<int> $a
             *  @param list<int> $b
             */
            function f(array $a, array $b): void {
                assertType('list<int>', $a + $b);
            }
        