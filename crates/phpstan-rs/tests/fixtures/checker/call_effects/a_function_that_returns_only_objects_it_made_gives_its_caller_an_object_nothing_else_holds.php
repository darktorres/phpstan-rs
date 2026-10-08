<?php
namespace CallEffects\FreshResults;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?int $id = null;
}

final class Registry
{
    /** @var list<Box> */
    public static array $all = [];

    public static function make(): Box
    {
        return new Box();
    }

    public static function viaMake(): Box
    {
        return Registry::make();
    }
}

function wild(string $name): void
{
    $name();
}

function makeBox(): Box
{
    $box = new Box();
    $box->id = 1;
    return $box;
}

function viaMakeBox(): Box
{
    return makeBox();
}

function sharedBox(): Box
{
    static $box;
    $box ??= new Box();
    return $box;
}

function leakedBox(): Box
{
    $box = new Box();
    Registry::$all[] = $box;
    return $box;
}

function handedBack(Box $box): Box
{
    return $box;
}

function aFunctionThatReturnsWhatItMade(): void
{
    $box = makeBox();
    $box->id = 5;
    wild('anything');
    assertType('5', $box->id);
}

function aFunctionThatReturnsWhatAnotherMade(): void
{
    $box = viaMakeBox();
    $box->id = 5;
    wild('anything');
    assertType('5', $box->id);
}

function aStaticMethodThatReturnsWhatItMade(): void
{
    $box = Registry::make();
    $box->id = 5;
    wild('anything');
    assertType('5', $box->id);
}

function aStaticMethodThatReturnsWhatAnotherMade(): void
{
    $box = Registry::viaMake();
    $box->id = 5;
    wild('anything');
    assertType('5', $box->id);
}

function aFunctionThatReturnsAStaticVariable(): void
{
    $box = sharedBox();
    $box->id = 5;
    wild('anything');
    assertType('int|null', $box->id);
}

function aFunctionThatStoredWhatItReturns(): void
{
    $box = leakedBox();
    $box->id = 5;
    wild('anything');
    assertType('int|null', $box->id);
}

function aFunctionThatReturnsItsArgument(Box $given): void
{
    $box = handedBack($given);
    $box->id = 5;
    wild('anything');
    assertType('int|null', $box->id);
}
