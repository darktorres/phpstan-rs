<?php
declare(strict_types=1);

use function PHPStan\Testing\assertType;

final class StrictStringLengthEffectBox
{
    public ?string $value = null;
}

final class StrictStringLengthEffectStringable
{
    public function __construct(private StrictStringLengthEffectBox $box) {}

    public function __toString(): string
    {
        $this->box->value = null;
        return 'value';
    }
}

function strlenStrictStringableDoesNotInvokeConversion(
    StrictStringLengthEffectBox $box,
    StrictStringLengthEffectStringable $value,
): void {
    if ($box->value !== null) {
        try {
            strlen($value);
        } catch (TypeError) {
        }
        assertType('string', $box->value);
    }
}
