<?php
/** @param array<int, string> $a */
function f(array $a): void {
    if (!array_is_list($a)) {
        \PHPStan\Testing\assertType('non-empty-array<int, string>', $a);
    } else {
        \PHPStan\Testing\assertType('list<string>', $a);
    }
    if (array_is_list($a)) {
        return;
    }
    \PHPStan\Testing\assertType('non-empty-array<int, string>', $a);
}
