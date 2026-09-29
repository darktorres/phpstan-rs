<?php
use function PHPStan\Testing\assertType;
function f(int $a, int $b): void {
    assertType('int', random_int($a, $b));
    assertType('5', random_int(5, 5));
    assertType('int<1, 10>', random_int(1, 10));
}
