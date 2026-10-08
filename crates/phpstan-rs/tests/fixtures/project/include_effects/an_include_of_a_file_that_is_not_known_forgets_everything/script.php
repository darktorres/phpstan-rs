<?php

use function PHPStan\Testing\assertType;

class Counter
{
    public static ?int $n = null;
}

function load(string $path): void
{
    Counter::$n = 1;
    require $path;
    assertType('int|null', Counter::$n);
}
