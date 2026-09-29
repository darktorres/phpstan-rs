<?php
use function PHPStan\Testing\assertType;
function f(int $i): void {
    $s = match ($i) { 1 => 'a', 2 => 'b', 3 => 'c', 4 => 'd', 5 => 'e', 6 => 'f', 7 => 'g', 8 => 'h', 9 => 'i', 10 => 'j', 11 => 'k', 12 => 'l', 13 => 'm', default => 'n' };
    assertType('string', $s);
    $n = match ($i) { 1 => 1, 2 => 2, 3 => 3, 4 => 4, 5 => 5, 6 => 6, 7 => 7, 8 => 8, 9 => 9, 10 => 10, 11 => 11, 12 => 12, 13 => 13, default => 'x' };
    assertType("int|'x'", $n);
}
