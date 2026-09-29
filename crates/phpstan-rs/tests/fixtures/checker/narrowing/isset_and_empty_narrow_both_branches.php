<?php
use function PHPStan\Testing\assertType;
/**
 * @param non-empty-string $ne
 * @param int<1, 5> $pos
 * @param list<int> $l
 */
function f(?int $i, int|string|null $j, string $ne, int $pos, array $l, mixed $m, bool $b, \stdClass $o): void {
    if (!isset($i)) { assertType('null', $i); } else { assertType('int', $i); }
    if (empty($j)) { assertType("null|0|''|'0'", $j); } else { assertType('int|non-falsy-string', $j); }
    if (empty($ne)) { assertType("'0'", $ne); }
    if (empty($l)) { assertType('array{}', $l); }
    if (empty($b)) { assertType('false', $b); }
    if (empty($m)) { assertType("null|false|0|0.0|''|'0'|array{}|SimpleXMLElement", $m); }
    if (!isset($undefinedVar)) { echo 1; }
}
/**
 * @param numeric-string $n
 * @param lowercase-string $lo
 * @param non-falsy-string $nf
 * @param int<0, 5> $z
 * @param array<string, int> $a
 */
function g(string $n, string $lo, string $nf, int $z, array $a, float $f, ?\SimpleXMLElement $x): void {
    if (empty($n)) { assertType("'0'", $n); }
    if (empty($lo)) { assertType("''|'0'", $lo); }
    if (empty($z)) { assertType('0', $z); }
    if (empty($a)) { assertType('array{}', $a); }
    if (empty($f)) { assertType('0.0', $f); }
    if (empty($x)) { assertType('null|SimpleXMLElement', $x); }
}
