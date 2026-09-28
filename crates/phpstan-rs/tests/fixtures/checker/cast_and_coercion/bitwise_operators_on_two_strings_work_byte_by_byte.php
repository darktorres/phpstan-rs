<?php
use function PHPStan\Testing\assertType;
/**
 * @param non-empty-string $ne
 * @param 'ab'|'cd' $two
 */
function f(string $s, string $ne, string $two, int $i, int|string $is, float $fl, mixed $m): void {
    assertType("'c'", 'a' ^ "\x02");
    assertType("'a'", 'ab' & 'a');
    assertType("'ab'", 'ab' | 'a');
    assertType("''", '' | '');
    assertType('string', $s & $ne);
    assertType('non-empty-string', $s | $ne);
    assertType('non-empty-string', $ne ^ $ne);
    assertType("'ab'|'cd'", $two | '');
    assertType('int', $i & $i);
    assertType('int|string', $is | $s);
    assertType("'2'", ~"\xCD");
    assertType('string', ~$s);
    assertType('non-empty-string', ~$ne);
    assertType('int', ~$fl);
    assertType('int|string', ~$m);
    assertType('int|string', ~$is);
    $x = 'a';
    $x ^= "\x03";
    assertType("'b'", $x);
    assertType('int<0, 7>', $i & 7);
}
