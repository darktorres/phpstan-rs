<?php
declare(strict_types=0);

use function PHPStan\Testing\assertType;

final class StrlenNormalBox
{
    public ?string $value = null;
}

final class StrlenNormalWritingStringable
{
    public function __construct(private StrlenNormalBox $box) {}

    public function __toString(): string
    {
        $this->box->value = null;
        return 'value';
    }
}

function strlenNormalPathKeepsNarrowingAndCallbackWrites(
    StrlenNormalBox $box,
    StrlenNormalWritingStringable $value,
    string|null $candidate,
): void {
    if (is_string($candidate) && $box->value !== null) {
        $length = strlen($value);
        assertType('int<0, max>', $length);
        assertType('string', $candidate);
        assertType('string|null', $box->value);
    }
}
