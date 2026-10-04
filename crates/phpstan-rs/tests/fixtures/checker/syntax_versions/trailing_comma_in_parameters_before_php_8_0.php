<?php
function f(int $a, int $b,): int
{
    $g = function () use ($a, $b,): int {
        return $a + $b;
    };
    return $g();
}
