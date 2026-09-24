<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $x */
            function f(array $x): void {
                $x['a'] = [1, 2, 3];
                assertType("array<string, mixed>&hasOffsetValue('a', array{0: 1, 1: 2, 2: 3})", $x);
                $y = $x['a'];
                assertType('array{0: 1, 1: 2, 2: 3}', $y);
            }
        