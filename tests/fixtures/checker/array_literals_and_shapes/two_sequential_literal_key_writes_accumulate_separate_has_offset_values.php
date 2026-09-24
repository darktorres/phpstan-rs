<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $x */
            function f(array $x): void {
                $x['a'] = [1, 2, 3];
                $x['b'] = 'hi';
                $a = $x['a'];
                $b = $x['b'];
                assertType('array{0: 1, 1: 2, 2: 3}', $a);
                assertType("'hi'", $b);
            }
        