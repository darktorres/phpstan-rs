<?php
namespace CallEffects\TraitStatic;

use function PHPStan\Testing\assertType;

trait Counter
{
    public static ?int $count = null;
}

final class First
{
    use Counter;
}

final class Second
{
    use Counter;
}

function writeFirst(): void
{
    First::$count = 1;
}

function check(): void
{
    First::$count = null;
    Second::$count = null;
    writeFirst();
    assertType('int|null', First::$count);
    assertType('null', Second::$count);
}

check();
