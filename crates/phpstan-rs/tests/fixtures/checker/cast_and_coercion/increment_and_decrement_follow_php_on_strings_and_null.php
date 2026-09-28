<?php
use function PHPStan\Testing\assertType;
/**
 * @param numeric-string $num
 * @param non-empty-string $ne
 */
function f(string $s, string $num, string $ne, ?int $ni): void {
    $a = 'a'; $a++; assertType("'b'", $a);
    $z = 'Az'; $z++; assertType("'Ba'", $z);
    $z9 = 'z9'; $z9++; assertType("'aa0'", $z9);
    $d = 'a'; $d--; assertType("'a'", $d);
    $e = ''; $e++; assertType("'1'", $e);
    $e2 = ''; $e2--; assertType('-1', $e2);
    $n = '5'; $n++; assertType('6', $n);
    $n2 = ' 5.5'; $n2--; assertType('4.5', $n2);
    $x = '1e3'; $x++; assertType('1001', $x);
    $big = '9223372036854775807'; $big++; assertType('9223372036854776000', $big);
    $nl = null; $nl++; assertType('1', $nl);
    $nd = null; $nd--; assertType('null', $nd);
    $b = true; $b++; assertType('true', $b);
    $num++; assertType('int|float', $num);
    $s++; assertType('int|float|non-empty-string', $s);
    $ne--; assertType('int|float|non-empty-string', $ne);
    $ni++; assertType('int', $ni);
    $p = 'a'; assertType("'a'", $p++); assertType("'b'", $p);
    $q = 'a'; assertType("'b'", ++$q);
}
