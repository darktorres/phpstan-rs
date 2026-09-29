<?php
/**
 * @param int<0, 5> $a
 * @param int<10, 20> $b
 * @param non-empty-string $ne
 * @param numeric-string $num
 * @param non-empty-array<int> $nea
 * @param array<int> $ai
 * @param array{x: string} $sh
 * @param array<int, string> $ok
 */
function f(int $a, int $b, string $ne, string $num, array $nea, array $ai, array $sh, array $ok): void {
    if ($a === $b) {}
    if ($ne === '') {}
    if ($num === 'abc') {}
    if ($nea === []) {}
    if ($ai === $sh) {}
    if (1.5 === 2.5) {}
    if ($ok === ['a']) {}
}
