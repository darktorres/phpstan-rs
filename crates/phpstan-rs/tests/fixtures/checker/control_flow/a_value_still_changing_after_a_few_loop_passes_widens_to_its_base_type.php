<?php
use function PHPStan\Testing\assertType;
/** @param list<int> $l */
function f(array $l): void {
    $acc = 0;
    foreach ($l as $n) { $acc += 1; }
    assertType('int', $acc);
    $s = '';
    foreach ($l as $n) { $s .= 'a'; }
    assertType('string', $s);
    for ($i = 0; $i < 3; $i++) {
        assertType('int<0, 2>', $i);
    }
    assertType('3', $i);
}
