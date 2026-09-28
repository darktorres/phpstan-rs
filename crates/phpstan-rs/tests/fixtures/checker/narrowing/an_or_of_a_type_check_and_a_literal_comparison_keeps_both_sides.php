<?php
use function PHPStan\Testing\assertType;
function f(?string $x): void {
    if (null === $x || $x === 'a') { assertType("'a'|null", $x); }
    if ($x === 'a' || null === $x) { assertType("null|'a'", $x); }
    if (is_null($x) || $x === 'a') { assertType("null|'a'", $x); }
}
function g(int|string $x): void {
    if ($x === 1 || $x === 'a') { assertType("1|'a'", $x); }
    if (is_int($x) || $x === 'a') { assertType("int|'a'", $x); }
}
function h(string $hookSpec): ?string {
    $paramIndex = null;
    if (false !== $hashPos = strpos($hookSpec, ')#')) {
        $paramIndex = substr($hookSpec, $hashPos + 2);
    }
    if ('get()' === $hookSpec && (null === $paramIndex || $paramIndex === (string) (int) $paramIndex)) {
        if (null !== $paramIndex) {
            return $paramIndex;
        }
    }
    return null;
}
