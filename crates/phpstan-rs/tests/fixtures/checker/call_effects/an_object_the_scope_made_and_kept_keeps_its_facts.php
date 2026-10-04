<?php
namespace CallEffects\Escape;

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

function clearEveryBox(): void
{
    foreach (Store::$boxes as $box) {
        $box->value = null;
    }
}

function keep(Box $box): void
{
    Store::$boxes[] = $box;
}

function kept(): void
{
    $box = new Box();
    $box->value = 1;
    clearEveryBox();
    assertType('1', $box->value);
}

function escaped(): void
{
    $box = new Box();
    keep($box);
    $box->value = 1;
    clearEveryBox();
    assertType('int|null', $box->value);
}

kept();
escaped();
