<?php
namespace CallEffects\StableProperties\Hooked;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value {
        set {
            $this->value = $value;
        }
    }

    public ?string $plain;

    public function __construct(?string $value)
    {
        $this->value = $value;
        $this->plain = $value;
    }
}

function aHookedProperty(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
    if ($box->plain !== null) {
        $callback();
        assertType('string', $box->plain);
    }
}
