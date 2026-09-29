<?php
use function PHPStan\Testing\assertType;
/**
 * @param int<0, 5> $r
 * @param int<-3, 0> $n
 * @param int<2, 4> $p
 * @param list<int> $l
 * @param 0.0|1.5 $fl
 */
function c3(bool $b, int $i, int $r, int $n, int $p, array $l, float $fl, ?string $s): void {
    if ($b) { assertType('true', $b); }
    if ($i) { assertType('int<min, -1>|int<1, max>', $i); }
    if ($r) { assertType('int<1, 5>', $r); }
    if ($n) { assertType('int<-3, -1>', $n); }
    if ($p) { assertType('int<2, 4>', $p); }
    if ($l) { assertType('non-empty-list<int>', $l); }
    if ($fl) { assertType('1.5', $fl); }
    if ($s) { assertType('non-falsy-string', $s); }
}
