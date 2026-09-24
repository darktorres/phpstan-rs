<?php
/** @param list<int> $ints */
function f(array $ints, array $unknown): void {
    array_map(fn ($i) => $i * 2, $ints);
    array_map(function ($i) { return $i; }, $ints);
    array_map(fn (int $i) => $i, $ints);
    array_map(fn ($x) => $x, $unknown);
}
