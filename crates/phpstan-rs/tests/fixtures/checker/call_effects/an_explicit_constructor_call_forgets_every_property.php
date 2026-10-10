<?php
namespace CallEffects\StableProperties\Reinitialised;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }

    public function reset(): void
    {
        $this->__construct(null);
    }
}

function aConstructorRunAgain(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
