<?php
namespace CallEffects\ElementWrites;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?int $value = null;
}

final class Store
{
    /** @var list<Box> */
    public static array $boxes = [];
}

function clearEveryStoredBox(): void
{
    foreach (Store::$boxes as $box) {
        $box->value = null;
    }
}

function clearTheFirstStoredBox(): void
{
    Store::$boxes[0]->value = null;
}

/** @param list<Box> $boxes */
function clearTheGivenBoxes(array $boxes): void
{
    foreach ($boxes as $box) {
        $box->value = null;
    }
}

function aLoopOverAStaticArray(Box $box): void
{
    $box->value = 1;
    clearEveryStoredBox();
    assertType('int|null', $box->value);
}

function anElementOfAStaticArray(Box $box): void
{
    $box->value = 1;
    clearTheFirstStoredBox();
    assertType('int|null', $box->value);
}

/** @param list<Box> $boxes */
function aLoopOverAnArrayParameter(Box $box, array $boxes): void
{
    $box->value = 1;
    clearTheGivenBoxes($boxes);
    assertType('int|null', $box->value);
}

function aCallThatWritesNoBox(Box $box): void
{
    $box->value = 1;
    echo count(Store::$boxes);
    assertType('1', $box->value);
}
