<?php
namespace CallEffects\StaticStorage;

use function PHPStan\Testing\assertType;

class Base
{
    public static ?int $shared = null;
    public static ?int $own = null;
}

final class Inherits extends Base
{
}

final class Redeclares extends Base
{
    public static ?int $own = null;
}

function writeInherited(): void
{
    Inherits::$shared = 1;
}

function writeRedeclared(): void
{
    Redeclares::$own = 1;
}

function check(): void
{
    Base::$shared = null;
    Base::$own = null;
    writeInherited();
    assertType('int|null', Base::$shared);
    writeRedeclared();
    assertType('null', Base::$own);
}

check();
