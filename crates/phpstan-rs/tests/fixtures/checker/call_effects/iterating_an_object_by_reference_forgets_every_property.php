<?php
namespace CallEffects\StableProperties\Iterated;

use function PHPStan\Testing\assertType;

final class Box
{
    public ?string $value;

    public function __construct(?string $value)
    {
        $this->value = $value;
    }
}

function scramble(mixed $subject): void
{
    foreach ($subject as &$member) {
        $member = null;
    }
}

function anObjectIteratedByReference(Box $box, callable $callback): void
{
    if ($box->value !== null) {
        $callback();
        assertType('string|null', $box->value);
    }
}
