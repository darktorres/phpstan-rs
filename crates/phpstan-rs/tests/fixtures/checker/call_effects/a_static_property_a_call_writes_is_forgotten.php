<?php
namespace CallEffects\StaticProperty;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
    public static int $other = 0;
}

function bump(): void
{
    Holder::$value = 5;
}

function check(): void
{
    Holder::$value = null;
    Holder::$other = 1;
    assertType('null', Holder::$value);
    bump();
    assertType('int|null', Holder::$value);
    assertType('1', Holder::$other);
}

check();
