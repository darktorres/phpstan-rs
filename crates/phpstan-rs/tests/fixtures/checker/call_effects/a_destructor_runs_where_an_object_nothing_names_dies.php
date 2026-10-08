<?php
namespace CallEffects\TemporaryDestructors;

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

function make(): Resets
{
    return new Resets();
}

function aDiscardedNewExpression(): void
{
    Flag::$value = null;
    new Resets();
    assertType('int|null', Flag::$value);
}

function aDiscardedCallResult(): void
{
    Flag::$value = null;
    make();
    assertType('int|null', Flag::$value);
}

function aQuietDiscardedNewExpression(): void
{
    Flag::$value = null;
    new Quiet();
    assertType('null', Flag::$value);
}

function aForeachTargetIsOverwritten(): void
{
    $held = new Resets();
    Flag::$value = null;
    foreach ([1, 2] as $held) {
    }
    assertType('int|null', Flag::$value);
}

function aForeachOverATemporaryDropsItsElements(): void
{
    Flag::$value = null;
    foreach ([new Resets()] as $item) {
    }
    unset($item);
    assertType('int|null', Flag::$value);
}

function aParameterOfUnknownClassIsDropped(object $object): void
{
    Flag::$value = null;
    $object = null;
    assertType('int|null', Flag::$value);
}

function aTemporaryReceiver(): void
{
    Flag::$value = null;
    make()->__destruct();
    assertType('int|null', Flag::$value);
}

function aCalleeKeepsALocalUntilItReturns(): void
{
    Flag::$value = null;
    $local = new Resets();
    assertType('null', Flag::$value);
}

function theCallerSeesTheLocalDie(): void
{
    Flag::$value = null;
    aCalleeKeepsALocalUntilItReturns();
    assertType('int|null', Flag::$value);
}
