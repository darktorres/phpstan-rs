<?php
namespace CallEffects\StableProperties\Assumed;

use function PHPStan\Testing\assertType;

final class Settings
{
    private ?string $secret;
    public ?string $label;

    public function __construct(?string $secret)
    {
        $this->secret = $secret;
        $this->label = $secret;
    }

    public function check(callable $callback): void
    {
        if ($this->secret !== null) {
            $callback();
            assertType('string', $this->secret);
        }
        if ($this->label !== null) {
            $callback();
            assertType('string|null', $this->label);
        }
    }
}

function fill(Settings $settings): void
{
    (new \ReflectionProperty($settings, 'secret'))->setValue($settings, null);
}
