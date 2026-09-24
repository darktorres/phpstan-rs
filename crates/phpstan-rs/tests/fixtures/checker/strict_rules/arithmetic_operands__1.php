<?php
/** @param numeric-string $num */
function f(int $i, float $f, string $s, ?int $maybe, bool $b, array $a, mixed $m, string $num, \GMP $g): void {
    $x = $i + $f;
    $x = $i + $s;
    $x = $maybe * 2;
    $x = $num / 2;
    $x = $a + $a;
    $x = $a + 1;
    $x = $m - 1;
    $x = -$b;
    $s++;
    $s--;
    $maybe++;
    $g++;
    $i **= $s;
    $x = '5' % 2;
}
