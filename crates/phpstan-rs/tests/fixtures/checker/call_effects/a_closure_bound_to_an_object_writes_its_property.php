<?php
namespace CallEffects\StableProperties\Bound2;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }
}

function empty_it(Box $box): void
{
    $bound = \Closure::bind(function (): void {
        $this->value = null;
    }, $box, Box::class);
    if ($bound !== null) {
        $bound();
    }
}

function aBoundClosure(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
