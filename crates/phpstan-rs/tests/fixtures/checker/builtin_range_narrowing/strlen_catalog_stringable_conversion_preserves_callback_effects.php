<?php
declare(strict_types=0);

use function PHPStan\Testing\assertType;

final class StringLengthEffectBox
{
    public ?string $value = null;
}

final class StringLengthEffectStringable
{
    public function __construct(private StringLengthEffectBox $box) {}

    public function __toString(): string
    {
        $this->box->value = null;
        return 'value';
    }
}

function strlenCatalogStillAppliesStringableEffects(
    StringLengthEffectBox $box,
    StringLengthEffectStringable $value,
): void {
    if ($box->value !== null) {
        $length = strlen($value);
        assertType('int<0, max>', $length);
        assertType('string|null', $box->value);
    }
}
