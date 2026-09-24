<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $x */
            function f(array $x, bool $cond): void {
                if ($cond) {
                    $x['a'] = [1, 2, 3];
                } else {
                    $x['a'] = 'str';
                }
                $a = $x['a'];
                assertType("array{0: 1, 1: 2, 2: 3}|'str'", $a);
            }
        