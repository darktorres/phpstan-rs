<?php

use function PHPStan\Testing\assertType;

function load(): void
{
    $written = 1;
    $kept = 'x';
    require __DIR__ . '/writes_one.php';
    assertType('mixed', $written);
    assertType('\'x\'', $kept);
}
