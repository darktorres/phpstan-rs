<?php
final class Name { public function __toString(): string { return ''; } }
function f(string $s, ?string $ns, int $i, array $a, Name $n, \stdClass $o, mixed $m): void {
    $x = $s . $i;
    $x = $s . $n;
    $x = $s . $o;
    $x = $s + $i;
    $x = $s == $i;
    $x = $s < $i;
    $x = $s === $i;
    $x = $s . $ns;
    $x = $a + $i;
    $x = $a + $a;
    $x = $s . $m;
    $s .= $i;
    $s += $i;
    $x = '' . $i;
}
