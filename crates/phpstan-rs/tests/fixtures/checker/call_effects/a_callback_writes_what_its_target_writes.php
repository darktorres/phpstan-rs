<?php
namespace CallEffects\Callback;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

function record(int $item): int
{
    Holder::$value = $item;
    return $item;
}

function identity(int $item): int
{
    return $item;
}

function check(): void
{
    Holder::$value = null;
    echo count(array_map('CallEffects\Callback\identity', [1, 2]));
    assertType('null', Holder::$value);
    echo count(array_map(static fn (int $item): int => $item, [1, 2]));
    assertType('null', Holder::$value);
    echo count(array_map('CallEffects\Callback\record', [1, 2]));
    assertType('int|null', Holder::$value);
}

check();
