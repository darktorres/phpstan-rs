<?php
use function PHPStan\Testing\assertType;
function f(int $a, int $b): void {
    assertType("non-falsy-lowercase-string", "sum: " . $a + $b);
    assertType("'x4'", "x" . 1 << 2);
    assertType("1|2", $a . 1 + 2 === "x" ? 1 : 2);
}
