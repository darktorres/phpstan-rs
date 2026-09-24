<?php
function f(int $i, ?int $n, string $s, array $a, \stdClass $o, bool $b, mixed $m): void {
    $x = (int) $i;
    $x = (int) $n;
    $x = (string) $s;
    $x = (string) $i;
    $x = (array) $a;
    $x = (object) $o;
    $x = (bool) $b;
    $x = (int) $m;
    $x = (int) 5;
}
