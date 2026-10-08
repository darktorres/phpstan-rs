<?php

use function PHPStan\Testing\assertType;

class Counter
{
    public static ?int $n = null;
    public static ?int $m = null;
}

function kept(): void
{
    Counter::$m = 1;
    require __DIR__ . '/outer.php';
    assertType('1', Counter::$m);
}

function written(): void
{
    Counter::$n = 1;
    require __DIR__ . '/outer.php';
    assertType('int|null', Counter::$n);
}
