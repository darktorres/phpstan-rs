<?php
namespace CallEffects\StableProperties\FromTrait;

use function PHPStan\Testing\assertType;

trait Clears
{
    public function clear(): void
    {
        $this->value = null;
    }
}

final class Box
{
    use Clears;

    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }
}

function aTraitMethodWrites(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
