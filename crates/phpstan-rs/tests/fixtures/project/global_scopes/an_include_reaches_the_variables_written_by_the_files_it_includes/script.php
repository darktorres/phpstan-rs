<?php

use function PHPStan\Testing\assertType;

function load(): void
{
    $inner = 1;
    $kept = 'x';
    require __DIR__ . '/outer.php';
    assertType('mixed', $inner);
    assertType('\'x\'', $kept);
}
