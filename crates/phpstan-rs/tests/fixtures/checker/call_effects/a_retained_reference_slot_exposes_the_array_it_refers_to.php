<?php
namespace CallEffects\RetainedSlot;

use function PHPStan\Testing\assertType;

class Keep
{
    /** @var list<array<string, mixed>> */
    public static array $slots = [];
}

/** @param array<string, mixed> $slot */
function retain(array &$slot): void
{
    Keep::$slots[] = &$slot;
}

/** @param array<string, mixed> $options */
function afterTheCall(array $options, callable $callback): void
{
    retain($options);
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}

/** @param array<string, mixed> $options */
function inALoopBeforeTheCall(array $options, callable $callback): void
{
    for ($i = 0; $i < 3; $i++) {
        if (is_int($options['key'])) {
            $callback();
            assertType('mixed', $options['key']);
        }
        retain($options);
    }
}

/** @param array<string, mixed> $options */
function neverReferenced(array $options, callable $callback): void
{
    if (is_int($options['key'])) {
        $callback();
        assertType('int', $options['key']);
    }
}
