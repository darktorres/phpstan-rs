<?php
namespace CallEffects\StableProperties\ByRefArgument;

use function PHPStan\Testing\assertType;

function clear(mixed &$slot): void
{
    $slot = null;
}

final class Box
{
    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }

    public function reset(): void
    {
        clear($this->value);
    }
}

function aPropertyPassedByReference(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
