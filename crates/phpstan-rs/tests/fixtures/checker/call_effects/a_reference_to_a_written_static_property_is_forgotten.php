<?php
namespace CallEffects\Reference;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

function bump(): void
{
    Holder::$value = 5;
}

function check(): void
{
    $alias = &Holder::$value;
    $alias = null;
    bump();
    assertType('int|null', $alias);
}

check();
