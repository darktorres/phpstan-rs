<?php
function f(): void
{
    /** @var array{ $x */
    $x = 1;
    \PHPStan\Testing\assertType('1', $x);
}
