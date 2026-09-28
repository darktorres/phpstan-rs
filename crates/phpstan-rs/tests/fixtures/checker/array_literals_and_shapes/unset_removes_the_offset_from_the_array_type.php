<?php

use function PHPStan\Testing\assertType;

/**
 * @param list<int> $l
 * @param non-empty-array<string, int> $m
 * @param array<string, int> $g
 * @param 'x'|'y' $k
 */
function f(array $l, array $m, array $g, string $k, int $i): void
{
    $a = ['x' => 1, 'y' => 2];
    unset($a['x']);
    assertType('array{y: 2}', $a);
    $c = [1, 2, 3];
    unset($c[0]);
    assertType('array{1: 2, 2: 3}', $c);
    unset($l[0]);
    assertType('array<int<0, max>, int>', $l);
    unset($m['a']);
    assertType('array<string, int>', $m);
    $g['k'] = 1;
    unset($g['k']);
    assertType('array<string, int>', $g);
    $n = ['a' => ['b' => 1, 'c' => 2]];
    unset($n['a']['b']);
    assertType('array{a: array{c: 2}}', $n);
    $o = ['x' => 1, 'y' => 2, 3 => 4];
    unset($o[$k]);
    assertType('array{x?: 1, y?: 2, 3: 4}', $o);
    $q = ['x' => 1, 3 => 4];
    unset($q[$i]);
    assertType('array{x: 1, 3?: 4}', $q);
    unset($undefined['x']);
    $s = ['x' => 1, 'y' => 2];
    if ($s['y'] === 2) {
        unset($s['x']);
        assertType('2', $s['y']);
    }
}
