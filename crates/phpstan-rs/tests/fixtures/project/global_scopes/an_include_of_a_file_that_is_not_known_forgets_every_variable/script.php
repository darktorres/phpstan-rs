<?php

use function PHPStan\Testing\assertType;

function load(string $path): void
{
    $kept = 'x';
    require $path;
    assertType('mixed', $kept);
}
