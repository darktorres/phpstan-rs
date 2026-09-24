<?php
            use function PHPStan\Testing\assertType;
            /** @param array<int, string> $a
             *  @param list<string> $b
             */
            function f(array $a, array $b): void {
                assertType('list<string>', array_merge($a, $b));
            }
        