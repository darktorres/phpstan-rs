<?php

use function PHPStan\Testing\assertType;

class Counter
{
    public static ?int $n = null;
    public static ?int $m = null;
}

function loader(): void
{
    require __DIR__ . '/set.php';
}

function kept(): void
{
    Counter::$m = 1;
    loader();
    assertType('1', Counter::$m);
}

function written(): void
{
    Counter::$n = 1;
    loader();
    assertType('int|null', Counter::$n);
}
