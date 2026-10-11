<?php
declare(strict_types=0);

use function PHPStan\Testing\assertType;

final class StrlenCatchThrowingStringable
{
    public function __toString(): string
    {
        throw new RuntimeException('conversion failed');
    }
}

final class StrlenCatchBox
{
    public ?string $value = null;
}

final class StrlenCatchWritingThrowingStringable
{
    public function __construct(private StrlenCatchBox $box) {}

    public function __toString(): string
    {
        $this->box->value = 'written';
        throw new RuntimeException('conversion failed');
    }
}

function strlenCatchKeepsIsStringNarrowing(
    StrlenCatchThrowingStringable $value,
    string|null $candidate,
): void {
    if (is_string($candidate)) {
        try {
            strlen($value);
        } catch (RuntimeException $error) {
            assertType('string', $candidate);
            assertType('RuntimeException', $error);
        }
    }
}

function strlenCatchKeepsNotNullNarrowing(
    StrlenCatchThrowingStringable $value,
    ?string $candidate,
): void {
    if ($candidate !== null) {
        try {
            strlen($value);
        } catch (Throwable) {
            assertType('string', $candidate);
        }
    }
}

function strlenCatchKeepsIntRangeNarrowing(
    StrlenCatchThrowingStringable $value,
    int $count,
): void {
    if ($count >= 0) {
        try {
            strlen($value);
        } catch (Throwable) {
            assertType('int<0, max>', $count);
        }
    }
}

function strlenCatchSeesResultBeforeAssignment(
    StrlenCatchThrowingStringable $value,
): void {
    $length = 'before';
    try {
        $length = strlen($value);
    } catch (Throwable) {
        assertType("'before'", $length);
    }
    assertType("int<0, max>|'before'", $length);
}

function strlenCatchJoinsCallbackWritesWithTypeErrorRoute(
    StrlenCatchBox $box,
    mixed $value,
): void {
    if ($value !== null) {
        $box->value = 'before';
        try {
            strlen($value);
        } catch (TypeError | RuntimeException) {
            assertType('string|null', $box->value);
        }
    }
}

function strlenCatchJoinsInvokedAndWithoutInvocationRoutes(
    mixed $value,
    ?string $candidate,
): void {
    if ($candidate !== null && $value !== null) {
        try {
            strlen($value);
        } catch (TypeError | RuntimeException $error) {
            assertType('string', $candidate);
            assertType('TypeError|RuntimeException', $error);
        }
    }
}
