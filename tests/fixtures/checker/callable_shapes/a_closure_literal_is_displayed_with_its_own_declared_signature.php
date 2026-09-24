<?php
            use function PHPStan\Testing\assertType;
            function test(): void {
                $f = function (int $x, string ...$rest): string { return ''; };
                assertType("Closure(int, string...): ''", $f);
                $g = fn (?int $y = null) => $y;
                assertType('Closure(int|null=): int|null', $g);
            }
        