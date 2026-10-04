<?php
class A
{
    public int $x = 1_000;
}
/** @param array<string, int> $b */
function g(array $b): int
{
    $f = fn($v) => $v;
    $a = [...[1], 2];
    $b["k"] ??= 1;
    return $f(1) + count($a) + $b["k"];
}
