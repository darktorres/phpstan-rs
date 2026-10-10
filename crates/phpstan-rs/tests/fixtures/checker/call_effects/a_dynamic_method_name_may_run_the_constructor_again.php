<?php
namespace CallEffects\StableProperties\DynamicReinitialisation;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }
}

function reinitialise(Box $box, string $method): void
{
    $box->$method('other');
}

function aComputedMethodNameMayRunTheConstructorAgain(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
