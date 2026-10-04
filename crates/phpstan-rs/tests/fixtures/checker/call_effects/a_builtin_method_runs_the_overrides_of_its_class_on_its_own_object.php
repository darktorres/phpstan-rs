<?php
namespace CallEffects\BuiltinOverrides;

use function PHPStan\Testing\assertType;

final class Holder
{
    public static ?int $value = null;
}

final class Loud extends \Exception
{
    public function __toString(): string
    {
        Holder::$value = 1;
        return 'loud';
    }
}

class Quiet extends \Exception
{
}

final class Below extends Quiet
{
    public function __toString(): string
    {
        Holder::$value = 1;
        return 'below';
    }
}

final class Alone extends \Exception
{
}

function onItsOwnClass(Loud $error): void
{
    Holder::$value = null;
    echo $error->getMessage();
    assertType('int|null', Holder::$value);
}

function onAClassBelow(Quiet $error): void
{
    Holder::$value = null;
    echo $error->getMessage();
    assertType('int|null', Holder::$value);
}

function withNoOverride(Alone $error): void
{
    Holder::$value = null;
    echo $error->getMessage();
    assertType('null', Holder::$value);
}

onItsOwnClass(new Loud());
onAClassBelow(new Below());
withNoOverride(new Alone());
