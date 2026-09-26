<?php
/**
 * @param list<int> $a
 * @param list<int> $b
 * @return list<int>|array<int, int>
 */
function f(array $a, array $b): array
{
    $cmp = fn (int $x, int $y): int => $x <=> $y;
    $one = array_udiff($a, $b, $cmp);
    $two = array_uintersect_uassoc($a, $b, $cmp, $cmp);
    $keys = array_diff_ukey($a, $b, 'strcmp');
    return $one + $two + $keys;
}
