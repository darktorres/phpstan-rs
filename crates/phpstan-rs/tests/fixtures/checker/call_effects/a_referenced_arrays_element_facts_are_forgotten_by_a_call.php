<?php
namespace CallEffects\ReferencedArrays;

use function PHPStan\Testing\assertType;

/** @param array<string, mixed> $out */
function fill(array &$out): void
{
}

/** @param array<string, mixed> $options */
function aByReferenceParameter(array &$options, callable $callback): void
{
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}

/** @param array<string, mixed> $options */
function anAliasOfAnElement(array $options, callable $callback): void
{
    $alias = &$options['other'];
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}

/** @param array<string, mixed> $options */
function aByReferenceArgument(array $options, callable $callback): void
{
    fill($options);
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}

/** @param array<string, mixed> $options */
function aByReferenceElementArgument(array $options, callable $callback): void
{
    preg_match('/x/', 'x', $options['match']);
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}

/** @param array<string, mixed> $options */
function aByReferenceLoop(array $options, callable $callback): void
{
    foreach ($options as &$value) {
    }
    unset($value);
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}

/** @param array<string, mixed> $options */
function aReferenceInAnArrayLiteral(array $options, callable $callback): void
{
    $holder = [&$options];
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}

/** @param array<string, mixed> $options */
function aVariableVariable(array $options, string $name, callable $callback): void
{
    $other = &$$name;
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}

/** @param \ArrayAccess<string, mixed> $options */
function anObjectIsNotAnArray(\ArrayAccess $options, callable $callback): void
{
    if (is_int($options['key'])) {
        $callback();
        assertType('mixed', $options['key']);
    }
}
