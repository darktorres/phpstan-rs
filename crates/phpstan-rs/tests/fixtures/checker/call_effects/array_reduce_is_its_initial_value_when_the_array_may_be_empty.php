<?php
namespace CallEffects\Reduce;

use function PHPStan\Testing\assertType;

function add(int|string|null $carry, int $item): int
{
    return (int) $carry + $item;
}

/**
 * @param list<int> $maybe
 * @param non-empty-list<int> $nonEmpty
 * @param callable(?int, int): int $callback
 */
function check(array $maybe, array $nonEmpty, callable $callback): void
{
    assertType('int|null', array_reduce($maybe, 'CallEffects\Reduce\add'));
    assertType("int|'x'", array_reduce($maybe, 'CallEffects\Reduce\add', 'x'));
    assertType('int', array_reduce($nonEmpty, 'CallEffects\Reduce\add', 'x'));
    assertType('int|null', array_reduce($maybe, $callback));
    assertType('int', array_reduce($nonEmpty, $callback, 0));
    assertType('int|null', array_reduce($maybe, add(...)));
    assertType('null', array_reduce([], static fn ($carry, $item) => 1));
    assertType("'x'", array_reduce([], static fn ($carry, $item) => 1, 'x'));
    assertType('int|null', array_reduce($maybe, static fn (?int $carry, int $item): int => $item));
    assertType('int', array_reduce($nonEmpty, static fn (?int $carry, int $item): int => $item));
    assertType(
        'array',
        array_reduce($nonEmpty, static function (array $carry, int $item): array {
            $carry[] = $item;
            return $carry;
        }, []),
    );
}

check([1, 2], [3], static fn (?int $carry, int $item): int => ($carry ?? 0) + $item);
