<?php
namespace CallEffects\Fields;

use function PHPStan\Testing\assertType;

final class Pair
{
    public ?int $first = null;
    public ?int $second = null;

    public function setFirst(int $value): void
    {
        $this->first = $value;
    }
}

function check(Pair $pair): void
{
    $pair->first = null;
    $pair->second = 2;
    $pair->setFirst(1);
    assertType('int|null', $pair->first);
    assertType('2', $pair->second);
}

check(new Pair());
