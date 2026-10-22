<?php
use function PHPStan\Testing\assertType;

function f(string $s, string $needle): void {
    assertType('int<0, 134217727>', strlen($s));
    assertType('int<1, 134217728>', strlen($s) + 1);
    assertType('int<0, 134217727>|false', strpos($s, $needle));
    assertType('int<0, 18014398241046529>', strlen($s) * strlen($needle));
}

f('abc', 'b');
