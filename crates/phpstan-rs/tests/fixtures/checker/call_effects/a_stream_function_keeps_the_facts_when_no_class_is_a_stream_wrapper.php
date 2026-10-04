<?php
namespace CallEffects\NoStreamWrapper;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

function check(): void
{
    Holder::$value = null;
    echo strlen((string) file_get_contents(__FILE__));
    assertType('null', Holder::$value);
}

check();
