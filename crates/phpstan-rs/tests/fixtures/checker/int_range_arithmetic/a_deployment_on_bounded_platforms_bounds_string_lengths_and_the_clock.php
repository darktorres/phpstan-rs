<?php
use function PHPStan\Testing\assertType;

function f(string $s, string $needle): void {
    assertType('int<0, 72057594037927935>', strlen($s));
    assertType('int<1, 72057594037927936>', strlen($s) + 1);
    assertType('int<0, 72057594037927935>|false', strpos($s, $needle));
    assertType('int<0, 9223372036>', time());
    assertType('int<3600, 9223375636>', time() + 3600);
}

f('abc', 'b');
