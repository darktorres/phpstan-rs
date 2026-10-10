<?php
namespace CallEffects\StableProperties\Reflective;

use function PHPStan\Testing\assertType;

final class Settings
{
    public ?string $label;

    public function __construct(?string $label)
    {
        $this->label = $label;
    }
}

function fill(Settings $settings): void
{
    (new \ReflectionProperty($settings, 'label'))->setValue($settings, null);
}

function aFileNamingAReflectiveWriterForgetsTheFact(Settings $settings, callable $callback): void
{
    if ($settings->label !== null) {
        $callback();
        assertType('string|null', $settings->label);
    }
}
