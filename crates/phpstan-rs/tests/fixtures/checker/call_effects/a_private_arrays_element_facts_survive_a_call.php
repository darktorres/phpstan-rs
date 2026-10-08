<?php
namespace CallEffects\PrivateArrays;

use function PHPStan\Testing\assertType;

/** @param array<string, mixed> $options */
function aCallbackCannotChangeAParameterArray(array $options, callable $callback): void
{
    if (is_int($options['key'])) {
        $callback();
        assertType('int', $options['key']);
    }
}

function aCallCannotChangeALocalArray(callable $callback): void
{
    $row = ['key' => rand() ? 1 : 'one'];
    if (is_int($row['key'])) {
        $callback();
        assertType('1', $row['key']);
    }
}

/** @param array<string, array<string, mixed>> $rows */
function aNestedElementIsKept(array $rows, callable $callback): void
{
    if (is_string($rows['a']['b'])) {
        $callback(new \stdClass());
        assertType('string', $rows['a']['b']);
    }
}

/** @param array<string, mixed> $copy */
function aByValueArgumentIsACopy(array $copy): void
{
}

/** @param array<string, mixed> $options */
function passingAnArrayByValueKeepsItsFacts(array $options, callable $callback): void
{
    aByValueArgumentIsACopy($options);
    if (is_int($options['key'])) {
        $callback();
        assertType('int', $options['key']);
    }
}

/** @param array<string, mixed> $options */
function aClosureCapturingByValueTakesACopy(array $options, callable $callback): void
{
    $read = function () use ($options): void {
        $options['key'] = 'changed';
    };
    if (is_int($options['key'])) {
        $read();
        $callback();
        assertType('int', $options['key']);
    }
}
