<?php

namespace CallEffects\ShadowedPush;

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

/**
 * @param array<int, mixed> $list
 */
function array_push(array &$list): void
{
    $list[] = new Resets();
}

function aShadowFillsAnArray(): void
{
    $list = [];
    array_push($list);
}

function theCallerSeesTheObjectTheShadowStoredDie(): void
{
    Flag::$value = null;
    aShadowFillsAnArray();
    assertType('int|null', Flag::$value);
}
