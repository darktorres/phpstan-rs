<?php
namespace CallEffects\ByReferenceDestroys;

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

/** @param array<int, mixed> $out */
function overwrite(array &$out): void
{
    $out = [];
}

/** @param array<int, mixed> $list */
function replaceFirst(array &$list): void
{
    $list[0] = 1;
}

/** @param array<int, mixed> $list */
function append(array &$list): void
{
    $list[] = 1;
}

function drop(?object &$object, bool $flag): void
{
    if ($flag) {
        $object = null;
    }
}

function unsetIt(?object &$object): void
{
    unset($object);
}

function rebind(?object &$object, bool $flag): void
{
    $other = null;
    if ($flag) {
        $object = &$other;
    }
}

/** @param array<int, mixed> $list */
function unsetFirst(array &$list): void
{
    unset($list[0]);
}

/** @param array<int, mixed> $out */
function overwriteByDefault(array &$out = []): void
{
    $out = [];
}

final class Holder
{
    /** @param array<int, mixed> $out */
    public function overwrite(array &$out): void
    {
        $out = [];
    }

    /** @param array<int, mixed> $out */
    public static function overwriteStatic(array &$out): void
    {
        $out = [];
    }
}

function anOverwrittenArrayOfLoudObjects(): void
{
    $list = [new Resets()];
    Flag::$value = null;
    overwrite($list);
    assertType('int|null', Flag::$value);
}

function anOverwrittenArrayOfQuietObjects(): void
{
    $list = [new Quiet()];
    Flag::$value = null;
    overwrite($list);
    assertType('null', Flag::$value);
}

function anOverwrittenArrayOfNumbers(): void
{
    $list = [1, 2];
    Flag::$value = null;
    overwrite($list);
    assertType('null', Flag::$value);
}

function anOverwrittenElementOfLoudObjects(): void
{
    $list = [new Resets()];
    Flag::$value = null;
    replaceFirst($list);
    assertType('int|null', Flag::$value);
}

function anOverwrittenElementOfQuietObjects(): void
{
    $list = [new Quiet()];
    Flag::$value = null;
    replaceFirst($list);
    assertType('null', Flag::$value);
}

function anAppendDestroysNothing(): void
{
    $list = [new Resets()];
    Flag::$value = null;
    append($list);
    assertType('null', Flag::$value);
}

function anOverwrittenLoudObject(): void
{
    $object = new Resets();
    Flag::$value = null;
    drop($object, true);
    assertType('int|null', Flag::$value);
}

function anOverwrittenQuietObject(): void
{
    $object = new Quiet();
    Flag::$value = null;
    drop($object, true);
    assertType('null', Flag::$value);
}

function anUnsetElementOfLoudObjects(): void
{
    $list = [new Resets()];
    Flag::$value = null;
    unsetFirst($list);
    assertType('int|null', Flag::$value);
}

function anUnsetElementOfQuietObjects(): void
{
    $list = [new Quiet()];
    Flag::$value = null;
    unsetFirst($list);
    assertType('null', Flag::$value);
}

function anUnsetParameterLeavesTheArgument(): void
{
    $object = new Resets();
    Flag::$value = null;
    unsetIt($object);
    assertType('null', Flag::$value);
}

function aReboundParameterLeavesTheArgument(): void
{
    $object = new Resets();
    Flag::$value = null;
    rebind($object, true);
    assertType('null', Flag::$value);
}

function aDefaultIsNoValueTheCallerHolds(): void
{
    Flag::$value = null;
    overwriteByDefault();
    assertType('null', Flag::$value);
}

function aMethodOverwritesLoudObjects(Holder $holder): void
{
    $list = [new Resets()];
    Flag::$value = null;
    $holder->overwrite($list);
    assertType('int|null', Flag::$value);
}

function aMethodOverwritesQuietObjects(Holder $holder): void
{
    $list = [new Quiet()];
    Flag::$value = null;
    $holder->overwrite($list);
    assertType('null', Flag::$value);
}

function aStaticMethodOverwritesLoudObjects(): void
{
    $list = [new Resets()];
    Flag::$value = null;
    Holder::overwriteStatic($list);
    assertType('int|null', Flag::$value);
}

function aStaticMethodOverwritesQuietObjects(): void
{
    $list = [new Quiet()];
    Flag::$value = null;
    Holder::overwriteStatic($list);
    assertType('null', Flag::$value);
}

/** @param array<int, mixed> $list */
function passesItOn(array &$list): void
{
    overwrite($list);
}

function anOverwriteThroughAnotherFunction(): void
{
    $list = [new Resets()];
    Flag::$value = null;
    passesItOn($list);
    assertType('int|null', Flag::$value);
}

function theCallerSeesTheOverwrittenArrayDie(): void
{
    Flag::$value = null;
    anOverwrittenArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}
