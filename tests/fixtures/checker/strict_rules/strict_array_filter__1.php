<?php
/**
 * @param list<int> $ints
 * @param list<bool> $bools
 * @param list<list<int>> $lists
 * @param list<\stdClass|null> $objects
 * @param list<int|null> $maybeInts
 */
function f(array $ints, array $bools, array $lists, array $objects, array $maybeInts, ?callable $cb): void {
    array_filter($ints);
    array_filter($bools);
    array_filter($lists);
    array_filter($objects);
    array_filter($maybeInts);
    array_filter($ints, null);
    array_filter($ints, $cb);
    array_filter($ints, fn (int $i): bool => $i > 0);
    array_filter($ints, mode: ARRAY_FILTER_USE_KEY);
}
