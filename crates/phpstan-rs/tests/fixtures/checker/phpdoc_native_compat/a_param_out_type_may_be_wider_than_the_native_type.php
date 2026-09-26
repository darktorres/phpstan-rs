<?php
/**
 * @param-out string $x
 */
function f(?int &$x): void
{
    $x = 'now a string';
}

function g(): void
{
    $v = 1;
    f($v);
    \PHPStan\Testing\assertType('string', $v);
}
