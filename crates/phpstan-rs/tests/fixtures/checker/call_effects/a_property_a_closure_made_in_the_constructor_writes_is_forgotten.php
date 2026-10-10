<?php
namespace CallEffects\StableProperties\Closure;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value;
    public \Closure $clear;

    public function __construct(?string $value)
    {
        $this->value = $value;
        $this->clear = function (): void {
            $this->value = null;
        };
    }
}

function aClosureWrites(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
