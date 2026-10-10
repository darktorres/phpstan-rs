<?php
namespace CallEffects\StableProperties;

use function PHPStan\Testing\assertType;

final class Settings
{
    public ?string $label;
    public ?int $count;
    public ?int $mutated;
    public ?string $other;

    public function __construct(?string $label)
    {
        $this->label = $label;
        $this->count = 0;
        $this->mutated = null;
        $this->other = $label;
    }

    public function __clone()
    {
        $this->count = 0;
    }

    public function bump(int $to): void
    {
        $this->mutated = $to;
    }
}

function aConstructorOnlyPropertyKeepsItsFact(Settings $settings, callable $callback): void
{
    if ($settings->label !== null) {
        $callback();
        assertType('string', $settings->label);
    }
}

function aPropertyAMethodWritesIsForgotten(Settings $settings, callable $callback): void
{
    if ($settings->mutated !== null) {
        $callback();
        assertType('int|null', $settings->mutated);
    }
}

function aPropertyWrittenThroughAnotherObjectIsForgotten(Settings $settings, Settings $other, callable $callback): void
{
    $other->other = null;
    if ($settings->other !== null) {
        $callback();
        assertType('string|null', $settings->other);
    }
}
