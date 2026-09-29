<?php
/** @param array{a: int, b: array{string}} $row */
function f(array $row): void {
    [$k => $a] = $row;
}
/** @param array{a: int, b: array{string}} $row */
function g(array $row): void {
    ['a' => $a, 'b' => [$s]] = $row;
    \PHPStan\Testing\assertType('int', $a);
    \PHPStan\Testing\assertType('string', $s);
    foreach ([$row] as ['b' => [$t]]) {
        \PHPStan\Testing\assertType('string', $t);
    }
}
