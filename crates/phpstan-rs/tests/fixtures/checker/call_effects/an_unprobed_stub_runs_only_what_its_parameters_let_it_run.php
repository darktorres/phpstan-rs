<?php
namespace CallEffects\UnprobedStubs;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $name = null;
}

final class Loud
{
    public function __construct(private Box $box)
    {
    }

    public function __toString(): string
    {
        $this->box->name = null;
        return 'loud';
    }
}

function aStubGivenScalarsRunsNoCode(Box $box): void
{
    if ($box->name !== null) {
        apcu_store('key', 1);
        assertType('string', $box->name);
    }
}

function aStubGivenAnObjectRunsItsConversion(Box $box, Loud $loud): void
{
    if ($box->name !== null) {
        apcu_store('key', $loud);
        assertType('string|null', $box->name);
    }
}
