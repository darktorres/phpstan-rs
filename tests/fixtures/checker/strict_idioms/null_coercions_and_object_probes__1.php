<?php
final class A {}
function f(?string $s, ?int $n, ?A $a, A $b, string $plain): void {
    echo "x{$s}y";
    echo $s . $plain;
    $total = $n + 1;
    $plain .= $s;
    isset($a);
    empty($b);
    echo "{$plain}" . $plain;
    isset($plain);
    $list = [new A()];
    isset($list[0]);
}
