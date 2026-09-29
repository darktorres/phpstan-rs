<?php
use function PHPStan\Testing\assertType;
const K = 'k';
/**
 * @param non-empty-string $ne
 * @param numeric-string $ns
 * @param list<int> $l
 * @param array<string, int> $m
 */
function f(string $s, string $ne, string $ns, int $i, array $l, array $m): void {
    assertType('array{-1: 1}', [-1 => 1]);
    assertType('array{-1: 1, 0: 2}', [-1 => 1, 2]);
    assertType('array{5: 1}', ['5' => 1]);
    assertType('array{1: 1, 0: 2, \'\': 3}', [true => 1, false => 2, null => 3]);
    assertType('array{k: 1}', [K => 1]);
    assertType('non-empty-array<(int|string), 1>', [$s => 1]);
    assertType('non-empty-array<(int|string), 1|2>', ['a' => 1, $s => 2]);
    assertType('non-empty-array<int, 1|2>', [$i => 1, 2]);
    assertType('non-empty-array<(int|non-empty-string), 1>', [$ne => 1]);
    assertType('non-empty-array<(int|numeric-string), 1>', [$ns => 1]);
    $b = [];
    $b[$s] = 1;
    assertType('non-empty-array<(int|string), 1>', $b);
    $c = [];
    $c['5'] = 1;
    assertType('array{5: 1}', $c);
    $d = [];
    $d[true] = 1;
    assertType('array{1: 1}', $d);
    assertType('non-empty-list<int>', [...$l, 1]);
    assertType('non-empty-array<string, int>', ['x' => 1, ...$m]);
    $dup = [1 => 'a', '1' => 'b', true => 'c', -1 => 'd', -1 => 'e'];
}
