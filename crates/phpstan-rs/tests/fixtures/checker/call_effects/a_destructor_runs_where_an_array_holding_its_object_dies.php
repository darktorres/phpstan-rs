<?php
namespace CallEffects\ArrayDestructors;

use function PHPStan\Testing\assertType;

final class Flag
{
    public static ?int $value = null;
}

final class Resets
{
    public function __destruct()
    {
        Flag::$value = 5;
    }
}

final class Quiet
{
    public function __destruct()
    {
    }
}

/** @return list<Resets> */
function makeList(): array
{
    return [new Resets()];
}

/** @return list<Quiet> */
function makeQuietList(): array
{
    return [new Quiet()];
}

function makeCount(): int
{
    return 1;
}

function anArrayLocalDiesWithTheFunction(): void
{
    $list = [new Resets()];
}

function theCallerSeesTheArrayDie(): void
{
    Flag::$value = null;
    anArrayLocalDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function anElementWrittenLaterDiesWithTheFunction(): void
{
    $list = [];
    $list[] = new Resets();
}

function theCallerSeesTheElementDie(): void
{
    Flag::$value = null;
    anElementWrittenLaterDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function aNestedArrayDiesWithTheFunction(): void
{
    $list = ['a' => [new Resets()]];
}

function theCallerSeesTheNestedArrayDie(): void
{
    Flag::$value = null;
    aNestedArrayDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function aDiscardedArrayLiteral(): void
{
    Flag::$value = null;
    [new Resets()];
    assertType('int|null', Flag::$value);
}

function anElementOfADiscardedCallResult(): void
{
    Flag::$value = null;
    makeList()[0];
    assertType('int|null', Flag::$value);
}

function aDiscardedCallResult(): void
{
    Flag::$value = null;
    makeList();
    assertType('int|null', Flag::$value);
}

function aCopyOfAListDiesWithTheFunction(): void
{
    $list = makeList();
    $copy = $list;
}

function theCallerSeesTheCopyDie(): void
{
    Flag::$value = null;
    aCopyOfAListDiesWithTheFunction();
    assertType('int|null', Flag::$value);
}

function anArrayOfQuietObjectsRunsNothing(): void
{
    Flag::$value = null;
    $list = [new Quiet()];
    makeQuietList();
    assertType('null', Flag::$value);
}

function anArrayOfNumbersRunsNothing(): void
{
    Flag::$value = null;
    $list = [1, 2, 3];
    $list[] = makeCount();
    [makeCount()];
    assertType('null', Flag::$value);
}
