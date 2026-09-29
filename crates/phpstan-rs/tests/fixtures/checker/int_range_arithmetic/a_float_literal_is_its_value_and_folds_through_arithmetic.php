<?php
use function PHPStan\Testing\assertType;
function f(float $x, int $i): void {
    assertType('1.7', 1.7);
    assertType('-2', ~1.7);
    $f = 0.5;
    assertType('1.0', $f * 2);
    assertType('0.5', 1 / 2);
    assertType('2.5', 5 / 2.0);
    assertType('1', 7.9 % 3);
    assertType('8.0', 2.0 ** 3);
    assertType('array{1: 1}', [1.7 => 1]);
    assertType('float', $x + 1.5);
    assertType('float', $i + 1.5);
    assertType('-1', 1.5 <=> 2);
    $acc = 0.0;
    foreach ([1, 2, 3] as $n) { $acc += 0.5; }
    assertType('float', $acc);
}
