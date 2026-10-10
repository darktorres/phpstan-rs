<?php

namespace CallEffects\ClosureReturn;

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

function aClosureReturningNothingIsCalled(): void
{
    $call = function (): void {
    };
    $call();
}

function theCallerSeesNothingRunForAVoidClosure(): void
{
    Flag::$value = null;
    aClosureReturningNothingIsCalled();
    assertType('null', Flag::$value);
}

function aClosureReturningANumberIsCalled(): void
{
    $call = fn(): int => 1;
    $call();
}

function theCallerSeesNothingRunForAnIntClosure(): void
{
    Flag::$value = null;
    aClosureReturningANumberIsCalled();
    assertType('null', Flag::$value);
}

function aClosureFillsAnArrayItShares(): void
{
    $list = [];
    $add = function () use (&$list): void {
        $list[] = new Quiet();
    };
    $add();
}

function theCallerSeesNothingRunForTheSharedArray(): void
{
    Flag::$value = null;
    aClosureFillsAnArrayItShares();
    assertType('null', Flag::$value);
}

function aClosureFillsAnArrayWithAnObjectThatRunsCode(): void
{
    $list = [];
    $add = function () use (&$list): void {
        $list[] = new Resets();
    };
    $add();
}

function theCallerSeesTheSharedArrayDie(): void
{
    Flag::$value = null;
    aClosureFillsAnArrayWithAnObjectThatRunsCode();
    assertType('int|null', Flag::$value);
}

function aClosureReturningAnObjectIsCalled(): void
{
    $make = function (): Resets {
        return new Resets();
    };
    $make();
}

function theCallerSeesTheReturnedObjectDie(): void
{
    Flag::$value = null;
    aClosureReturningAnObjectIsCalled();
    assertType('int|null', Flag::$value);
}

function aClosureDeclaringNothingIsCalled(): void
{
    $make = function () {
        return new Resets();
    };
    $make();
}

function theCallerSeesWhatAnUndeclaredClosureReturnedDie(): void
{
    Flag::$value = null;
    aClosureDeclaringNothingIsCalled();
    assertType('int|null', Flag::$value);
}

function aClosureReturningAnArrayIsCalled(): void
{
    $make = function (): array {
        return [new Resets()];
    };
    $make();
}

function theCallerSeesTheReturnedArrayDie(): void
{
    Flag::$value = null;
    aClosureReturningAnArrayIsCalled();
    assertType('int|null', Flag::$value);
}

function aParameterCallbackIsCalled(callable $call): void
{
    $call();
}

function theCallerSeesWhatACallbackReturnedDie(): void
{
    Flag::$value = null;
    aParameterCallbackIsCalled(function () {
        return new Resets();
    });
    assertType('int|null', Flag::$value);
}
