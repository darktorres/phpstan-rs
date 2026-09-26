<?php
/**
 * @param int<1, 9> $digit
 * @param non-empty-string $text
 */
function f(int $i, int $digit, bool $b, float $x, string $text): void
{
    \PHPStan\Testing\assertType("'5'", (string) 5);
    \PHPStan\Testing\assertType('decimal-int-string', (string) $i);
    \PHPStan\Testing\assertType('decimal-int-string', (string) $digit);
    \PHPStan\Testing\assertType("'1'|''", (string) $b);
    \PHPStan\Testing\assertType("''", (string) null);
    \PHPStan\Testing\assertType('non-empty-string', (string) $x);
    \PHPStan\Testing\assertType('non-empty-string', (string) $text);
    \PHPStan\Testing\assertType('int<1, 9>', (int) $digit);
    \PHPStan\Testing\assertType('1', (int) true);
    \PHPStan\Testing\assertType('0', (int) null);
    \PHPStan\Testing\assertType('int<0, 1>', (int) $b);
    \PHPStan\Testing\assertType('int', (int) $text);
    \PHPStan\Testing\assertType('float', (float) $x);
}
