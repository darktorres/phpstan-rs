<?php
use function PHPStan\Testing\assertType;

/**
 * @param array{a: 1} $arr
 * @param array<string, int> $map
 */
function f(array $arr, array $map, int $int): void {
    $arr[$int] = 2;
    assertType('array{a: 1, ...<int, 2>}', $arr);
    $map[] = 5;
    assertType('non-empty-array<int>', $map);
    $x = [];
    for ($i = 0; $i < 3; $i++) {
        $x[$i] = $i;
    }
    assertType('array<int, int<0, 2>>', $x);
}
