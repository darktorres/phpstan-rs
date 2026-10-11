<?php
declare(strict_types=0);

use function PHPStan\Testing\assertType;

final class StrlenInvalidateBox
{
    public ?string $value = null;
}

final class StrlenInvalidateThrowingStringable
{
    public function __construct(private StrlenInvalidateBox $box) {}

    public function __toString(): string
    {
        $this->box->value = null;
        throw new RuntimeException('conversion failed');
    }
}

final class StrlenInvalidateDestructorThrows
{
    public function __construct(private StrlenInvalidateBox $box) {}

    public function __toString(): string
    {
        $this->box->value = null;
        return 'value';
    }

    public function __destruct()
    {
        throw new RuntimeException('destructor failed');
    }
}

function strlenInvalidatedOnThrowPath(
    StrlenInvalidateBox $box,
    StrlenInvalidateThrowingStringable $value,
): void {
    if ($box->value !== null) {
        try {
            strlen($value);
        } catch (RuntimeException) {
            assertType('string|null', $box->value);
        }
    }
}

function strlenInvalidatedBeforeFinally(
    StrlenInvalidateBox $box,
    StrlenInvalidateThrowingStringable $value,
): void {
    if ($box->value !== null) {
        try {
            strlen($value);
        } catch (RuntimeException) {
        } finally {
            assertType('string|null', $box->value);
        }
    }
}

function strlenInvalidatedThroughNonMatchingInnerTry(
    StrlenInvalidateBox $box,
    StrlenInvalidateThrowingStringable $value,
): void {
    if ($box->value !== null) {
        try {
            try {
                strlen($value);
            } catch (LogicException) {
            }
        } catch (RuntimeException) {
            assertType('string|null', $box->value);
        }
    }
}

function strlenInvalidatedByTemporaryDestructor(StrlenInvalidateBox $box): void
{
    if ($box->value !== null) {
        try {
            strlen(new StrlenInvalidateDestructorThrows($box));
        } catch (RuntimeException) {
            assertType('string|null', $box->value);
        }
    }
}

function strlenInvalidatedThroughDynamicName(
    StrlenInvalidateBox $box,
    StrlenInvalidateThrowingStringable $value,
): void {
    $function = 'strlen';
    if ($box->value !== null) {
        try {
            $function($value);
        } catch (RuntimeException) {
            assertType('string|null', $box->value);
        }
    }
}
