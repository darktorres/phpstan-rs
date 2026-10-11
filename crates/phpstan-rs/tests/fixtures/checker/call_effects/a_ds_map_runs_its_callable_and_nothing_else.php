<?php
namespace CallEffects\DsMap;

use function PHPStan\Testing\assertType;

final class Tally
{
    public int $sets = 0;
}

/** @param \Ds\Vector<int> $values */
function mapsWithAPureCallable(\Ds\Vector $values, Tally $tally): void
{
    $tally->sets = 0;
    $values->map(static fn (int $value): int => $value * 2);
    assertType('0', $tally->sets);
}
