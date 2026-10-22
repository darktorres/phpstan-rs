<?php
use function PHPStan\Testing\assertType;

/** @param list<int> $l */
function f(array $l): void {
    $i = 0;
    foreach ($l as $a) {
        $i++;
    }
    assertType('int<0, max>|float', $i);
}

f([1, 2]);
