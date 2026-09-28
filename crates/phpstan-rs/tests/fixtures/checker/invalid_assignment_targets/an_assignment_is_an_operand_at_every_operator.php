<?php
use function PHPStan\Testing\assertType;
function f(?int $a, int $g, int $i): void {
    $r = $a ?? $b = 1;
    $s = 1 | $g = 2;
    assertType("2", $g);
    $t = 2 * $i = 3;
    assertType("3", $i);
    if (!is_dir("x") || !$dir = opendir("x")) {
        return;
    }
    assertType("resource", $dir);
    $v = (int) $w = "5";
    assertType("'5'", $w);
    if (false === $x = strpos("ab", "b")) {
        return;
    }
    assertType("int<0, max>", $x);
}
