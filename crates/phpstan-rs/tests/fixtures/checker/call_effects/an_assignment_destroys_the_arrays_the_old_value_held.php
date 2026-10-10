<?php
namespace CallEffects\AssignmentDestroys;

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

function aLocalArrayIsOverwritten(): void
{
    $list = [new Resets()];
    $list = [];
}

function theCallerSeesTheOverwrittenArrayDie(): void
{
    Flag::$value = null;
    aLocalArrayIsOverwritten();
    assertType('int|null', Flag::$value);
}

function aLocalArrayOfQuietObjectsIsOverwritten(): void
{
    $list = [new Quiet()];
    $list = [];
}

function theCallerSeesNothingRunForQuietObjects(): void
{
    Flag::$value = null;
    aLocalArrayOfQuietObjectsIsOverwritten();
    assertType('null', Flag::$value);
}

function aLocalArrayOfNumbersIsOverwritten(): void
{
    $list = [1, 2];
    $list = [3];
}

function theCallerSeesNothingRunForNumbers(): void
{
    Flag::$value = null;
    aLocalArrayOfNumbersIsOverwritten();
    assertType('null', Flag::$value);
}

function aNestedArrayIsOverwritten(): void
{
    $list = ['a' => [new Resets()]];
    $list = 1;
}

function theCallerSeesTheNestedArrayDie(): void
{
    Flag::$value = null;
    aNestedArrayIsOverwritten();
    assertType('int|null', Flag::$value);
}

function anOverwrittenArrayWasAlsoCopied(): void
{
    $list = [new Quiet()];
    $copy = $list;
    $list = [];
}

function theCallerSeesNothingRunForTheCopiedQuietArray(): void
{
    Flag::$value = null;
    anOverwrittenArrayWasAlsoCopied();
    assertType('null', Flag::$value);
}
