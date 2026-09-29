<?php
use function PHPStan\Testing\assertType;

/** @param list<int> $a */
function f(int $i, float $f, array $a, string $s): void {
    assertType('false', is_numeric("\u{a0}1"));
    assertType('true', is_numeric(" 1\n"));
    assertType('true', is_numeric($f));
    assertType('false', is_numeric($a));
    assertType('bool', is_numeric($s));
    if (is_int($i)) {
        echo $i;
    }
}
