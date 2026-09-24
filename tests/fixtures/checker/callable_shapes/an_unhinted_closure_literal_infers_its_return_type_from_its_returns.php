<?php
            use function PHPStan\Testing\assertType;
            function test(bool $b): void {
                $none = function () { echo 'x'; };
                assertType('Closure(): void', $none);
                $mixedReturns = function () use ($b) { if ($b) { return; } return 1; };
                assertType('Closure(): 1|null', $mixedReturns);
                $mayFallOff = function () use ($b) { if ($b) { return 'a'; } };
                assertType("Closure(): 'a'|null", $mayFallOff);
                $narrowed = function (int|string $v) { if (is_int($v)) { return $v; } return 'x'; };
                assertType("Closure(int|string): int|'x'", $narrowed);
                assertType("int|'x'", $narrowed(1));
            }
        