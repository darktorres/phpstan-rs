<?php

namespace CallEffects\StaticVariables;

use function PHPStan\Testing\assertType;

final class Flag
{
    public static ?int $value = null;
}

final class Quiet
{
    public function __destruct()
    {
    }
}

final class Resets
{
    public function __destruct()
    {
        Flag::$value = 1;
    }
}

function aStaticArrayIsAppendedANumber(): void
{
    static $kept = [];
    $kept[] = 1;
}

function theCallerSeesNothingRunForTheAppendedNumber(): void
{
    Flag::$value = null;
    aStaticArrayIsAppendedANumber();
    assertType('null', Flag::$value);
}

function aStaticCounterIsIncremented(): void
{
    static $n = 0;
    $n = $n + 1;
}

function theCallerSeesNothingRunForTheCounter(): void
{
    Flag::$value = null;
    aStaticCounterIsIncremented();
    assertType('null', Flag::$value);
}

function aStaticArrayIsAppendedAQuietObject(): void
{
    static $kept = [];
    $kept[] = new Quiet();
}

function theCallerSeesNothingRunForTheAppendedObject(): void
{
    Flag::$value = null;
    aStaticArrayIsAppendedAQuietObject();
    assertType('null', Flag::$value);
}

function aStaticArrayHoldingAnObjectIsReplaced(): void
{
    static $kept = [];
    $kept[] = new Resets();
    $kept = [];
}

function theCallerSeesTheReplacedArrayDie(): void
{
    Flag::$value = null;
    aStaticArrayHoldingAnObjectIsReplaced();
    assertType('int|null', Flag::$value);
}

function aStaticObjectIsReplaced(): void
{
    static $kept = null;
    $kept = new Resets();
}

function theCallerSeesTheReplacedObjectDie(): void
{
    Flag::$value = null;
    aStaticObjectIsReplaced();
    assertType('int|null', Flag::$value);
}

function aStaticIsWrittenThroughAReference(): void
{
    static $kept = 0;
    $alias = &$kept;
    $alias = new Resets();
}

function theCallerSeesTheObjectWrittenThroughTheReferenceDie(): void
{
    Flag::$value = null;
    aStaticIsWrittenThroughAReference();
    assertType('int|null', Flag::$value);
}

function aStaticHoldingAnObjectIsCalledOn(): void
{
    static $kept = null;
    $kept = new Resets();
    $kept = new Quiet();
}

function theCallerSeesTheObjectTheStaticHeldDie(): void
{
    Flag::$value = null;
    aStaticHoldingAnObjectIsCalledOn();
    assertType('int|null', Flag::$value);
}
