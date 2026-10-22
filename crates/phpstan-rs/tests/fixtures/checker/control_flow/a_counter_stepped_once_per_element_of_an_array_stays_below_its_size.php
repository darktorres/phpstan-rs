<?php
declare(strict_types=1);

use function PHPStan\Testing\assertType;

function offset(int $x): void {
}

/**
 * @param list<int> $l
 * @param iterable<int> $it
 */
function f(array $l, iterable $it): void {
    $idx = 0;
    foreach ($l as $v) {
        offset($idx * 50);
        $idx++;
    }
    assertType('int<0, 1073741824>', $idx);
    $pair = 0;
    foreach (['a' => 1, 'b' => 2] as $v) {
        $pair++;
    }
    assertType('int<1, 2>', $pair);
    $grown = 0;
    foreach ($l as &$v) {
        if ($v > 0) {
            $l[] = $v - 1;
        }
        offset($grown * 50);
        $grown++;
    }
    unset($v);
    assertType('int<0, 541784354586624000>', $grown);
    $t = 0;
    foreach ($it as $v) {
        $t++;
    }
    assertType('int<0, 541784354586624000>', $t);
    $n = 0;
    foreach ($l as $v) {
        while ($v-- > 0) {
            $n++;
        }
    }
    assertType('int<0, 541784354586624000>', $n);
    $g = 0;
    foreach ($l as $v) {
        again:
        $g++;
        if ($g % 2 === 1) {
            goto again;
        }
    }
    assertType('int<0, max>|float', $g);
}

f([1, 2], [3]);
