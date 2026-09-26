<?php
            use function PHPStan\Testing\assertType;
            function test(): void {
                $f = fn (int $x): string => (string) $x;
                assertType('decimal-int-string', $f(1));
                $g = fn (int $x) => $x > 0;
                assertType('Closure(int): bool', $g);
                assertType('bool', $g(1));
                $h = function (int $x) { return $x; };
                assertType('Closure(int): int', $h);
            }
        