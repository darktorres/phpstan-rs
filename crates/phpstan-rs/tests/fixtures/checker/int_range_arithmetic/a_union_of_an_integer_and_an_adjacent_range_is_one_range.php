<?php
use function PHPStan\Testing\assertType;
final class R {
    /** @param 0|positive-int $count */
    public function __construct(public int $count) {}
}
/** @param int<0, max> $n */
function f(int $n): R { return new R($n); }
/**
 * @param 0|positive-int $a
 * @param int<1, 5>|int<6, 9>|12 $b
 * @param 1|2|3 $c
 * @param int<min, 0>|int<1, max> $d
 * @param int<3, 5>|1 $e
 */
function g(int $a, int $b, int $c, int $d, int $e): void {
    assertType('int<0, max>', $a);
    assertType('int<1, 9>|12', $b);
    assertType('1|2|3', $c);
    assertType('int', $d);
    assertType('int<3, 5>|1', $e);
}
