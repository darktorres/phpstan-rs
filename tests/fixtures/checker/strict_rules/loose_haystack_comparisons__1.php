<?php
/**
 * @param list<string> $names
 * @param list<int> $ids
 * @param list<'a'|'b'> $letters
 * @param list<numeric-string> $numbers
 * @param list<?int> $maybe
 */
function f(array $names, array $ids, array $letters, array $numbers, array $maybe, int $n, string $s, mixed $m): void {
    in_array($s, $names);
    in_array($n, $ids);
    in_array($n, $letters);
    in_array($n, $names);
    in_array($s, $numbers);
    in_array($n, $maybe);
    in_array($n, $names, true);
    in_array($m, $names);
    array_search(1.5, $ids);
    array_keys($ids, '1');
}
