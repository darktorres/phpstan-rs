<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $x */
            function f(array $x): void {
                $x['a'] = 'first';
                $x['a'] = 'second';
                $a = $x['a'];
                assertType("'second'", $a);
            }
        