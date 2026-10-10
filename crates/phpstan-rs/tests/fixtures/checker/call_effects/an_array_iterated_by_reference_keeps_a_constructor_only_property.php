<?php
namespace CallEffects\StableProperties\IteratedArray;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }
}

/** @param list<int> $numbers */
function double(array $numbers): void
{
    foreach ($numbers as &$number) {
        $number *= 2;
    }
}

function anArrayIteratedByReference(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string', $box->value);
    }
}
