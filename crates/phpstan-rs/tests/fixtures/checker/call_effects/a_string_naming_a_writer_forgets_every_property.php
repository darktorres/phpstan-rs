<?php
namespace CallEffects\StableProperties\Named;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }
}

/**
 * @param list<string> $rows
 * @return array<int, mixed>
 */
function load(array $rows): array
{
    return array_map('unserialize', $rows);
}

function aCallableString(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
