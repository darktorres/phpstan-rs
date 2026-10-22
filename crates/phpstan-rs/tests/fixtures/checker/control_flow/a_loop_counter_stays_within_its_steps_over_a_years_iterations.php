<?php
use function PHPStan\Testing\assertType;

function bump(int &$x): void {
    $x *= 2;
}

/** @param list<int> $l */
function f(array $l, int $start, callable $g): void {
    $i = 0;
    foreach ($l as $a) {
        foreach ($l as $b) {
            $i++;
        }
    }
    assertType('int<0, 541784354586624000>', $i);
    $k = 10;
    foreach ($l as $a) {
        $k += 3;
        $k -= 2;
    }
    assertType('int<10, 3221225482>', $k);
    $d = 1;
    foreach ($l as $a) {
        $d *= 2;
    }
    assertType('int<1, max>|float', $d);
    $r = 0;
    foreach ($l as $a) {
        $r++;
        bump($r);
    }
    assertType('mixed', $r);
    $s = $start;
    foreach ($l as $a) {
        $s++;
    }
    assertType('int|float', $s);
    $c = 0;
    foreach ($l as $a) {
        $c++;
        $g($c);
    }
    assertType('int<0, max>|float', $c);
}

f([1, 2], 5, 'strlen');
