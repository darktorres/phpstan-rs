<?php
namespace CallEffects\BuiltinMethods;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

final class Counted implements \Countable
{
    public function count(): int
    {
        Holder::$value = 1;
        return 1;
    }
}

function check(Counted $counted): void
{
    Holder::$value = null;
    echo strlen('text');
    assertType('null', Holder::$value);
    echo count($counted);
    assertType('int|null', Holder::$value);
}

check(new Counted());
