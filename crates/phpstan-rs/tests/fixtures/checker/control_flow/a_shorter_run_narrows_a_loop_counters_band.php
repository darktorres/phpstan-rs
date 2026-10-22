<?php
use function PHPStan\Testing\assertType;

/** @param iterable<int> $l */
function f(iterable $l): void {
    $i = 0;
    foreach ($l as $a) {
        $i++;
    }
    assertType('int<0, 1484340697497600>', $i);
}

f([1, 2]);
