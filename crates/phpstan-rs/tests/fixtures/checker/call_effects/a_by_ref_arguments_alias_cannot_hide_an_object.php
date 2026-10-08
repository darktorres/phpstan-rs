<?php
namespace CallEffects\ByRefAlias;

use function PHPStan\Testing\assertType;

final class Writer
{
    public static ?int $seen = 1;

    public function __toString(): string
    {
        self::$seen = null;
        return '';
    }
}

function stash(array &$items): void
{
    $items[] = new Writer();
}

function aByRefArgumentTheCalleeFilledMayHoldAnObject(): void
{
    $items = [];
    stash($items);
    Writer::$seen = 1;
    implode(',', $items);
    assertType('int|null', Writer::$seen);
}

function anArrayNothingFilledHoldsNoObject(): void
{
    $items = [];
    Writer::$seen = 1;
    implode(',', $items);
    assertType('1', Writer::$seen);
}

function aByRefParameterTheBodyFilledMayHoldAnObject(array &$items): void
{
    $items[] = new Writer();
    Writer::$seen = 1;
    implode(',', $items);
    assertType('int|null', Writer::$seen);
}

/** @param array<int, int> $items */
function aByRefParameterOfIntegersHoldsNoObject(array &$items): void
{
    Writer::$seen = 1;
    implode(',', $items);
    assertType('1', Writer::$seen);
}
