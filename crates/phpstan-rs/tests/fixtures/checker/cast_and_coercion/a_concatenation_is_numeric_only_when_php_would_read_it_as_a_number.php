<?php
use function PHPStan\Testing\assertType;
/**
 * @param numeric-string $n
 * @param int<0, max> $u
 * @param '1 '|'2 ' $sp
 * @param '1'|'2' $d
 */
function f(string $n, int $i, int $u, string $sp, string $d): void {
    assertType('non-falsy-string', $n . '5');
    assertType('non-falsy-lowercase-uppercase-numeric-string', $i . '5');
    assertType('non-falsy-lowercase-uppercase-non-decimal-int-string', $i . '+5');
    assertType('non-falsy-lowercase-uppercase-non-decimal-int-string', $i . ' 5');
    assertType('non-falsy-lowercase-non-decimal-int-numeric-string', $i . 'e5');
    assertType('non-falsy-lowercase-uppercase-numeric-string', $d . $u);
    assertType('non-falsy-lowercase-uppercase-non-decimal-int-string', $sp . $u);
}
