<?php
namespace CallEffects\DecimalString;

use function PHPStan\Testing\assertType;

final class Tally
{
    public int $sets = 0;
}

function casts(\Decimal\Decimal $decimal, Tally $tally): void
{
    $tally->sets = 0;
    $text = (string) $decimal;
    assertType('0', $tally->sets);
    assertType('string', $text);
}
