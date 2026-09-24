<?php
            use function PHPStan\Testing\assertType;
            function f(int|string|bool $x): void {
                if ($x === 1 || $x === "a") {
                    assertType("1|'a'", $x);
                }
            }
        