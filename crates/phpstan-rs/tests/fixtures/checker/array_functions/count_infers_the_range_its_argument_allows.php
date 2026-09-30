<?php
use function PHPStan\Testing\assertType;

final class Five implements Countable
{
    /** @return int<1, 5> */
    public function count(): int
    {
        return 3;
    }
}

/**
 * @param array{a: int}|array{a: int, b?: int} $shapes
 * @param non-empty-array<string, int> $map
 * @param array<int, list<int>> $nested
 */
function f(array $shapes, array $map, Five $five, array $nested): void {
    assertType('int<1, 2>', count($shapes));
    assertType('int<1, max>', count($map));
    assertType('int<1, 5>', count($five));
    assertType('int<0, max>', count($nested, COUNT_RECURSIVE));
}

f(['a' => 1], ['k' => 1], new Five(), [[1]]);
