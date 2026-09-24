<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, callable> $a
             *  @param array<string, callable> $b
             */
            function f(array $a, array $b): void {
                $merged = $a + $b;
                assertType('array<string, callable>', $merged);
                foreach ($merged as $name => $callable) {
                    assertType('string', $name);
                }
            }
        