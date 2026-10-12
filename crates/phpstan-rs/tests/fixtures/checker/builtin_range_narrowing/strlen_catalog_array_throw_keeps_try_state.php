<?php
declare(strict_types=0);

use function PHPStan\Testing\assertType;

final class StrlenRouteBBox
{
    public ?string $value = null;
}

/** @param array<int, string> $value */
function strlenCatchArrayRouteKeepsTryState(
    array $value,
    StrlenRouteBBox $box,
): void {
    try {
        $box->value = 'before';
        // @phpstan-ignore-next-line argument.type
        strlen($value);
        $box->value = 'after';
    } catch (TypeError) {
        assertType("'before'", $box->value);
    }
}
