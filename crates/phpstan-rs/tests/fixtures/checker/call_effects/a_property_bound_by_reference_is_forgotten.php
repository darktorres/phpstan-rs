<?php
namespace CallEffects\StableProperties\Bound;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }

    public function alias(): void
    {
        $reference = &$this->value;
        $reference = null;
    }
}

function aReferenceToTheProperty(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
