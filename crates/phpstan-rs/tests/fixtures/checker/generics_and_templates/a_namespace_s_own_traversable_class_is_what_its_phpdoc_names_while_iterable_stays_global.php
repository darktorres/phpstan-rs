<?php
namespace App;

use function PHPStan\Testing\assertType;

final class Traversable
{
    public function mine(): int { return 1; }
}

/**
 * @param Traversable $own
 * @param iterable<int> $it
 * @param \Closure(Traversable): void $cb
 */
function f(Traversable $own, iterable $it, \Closure $cb): void
{
    assertType('App\Traversable', $own);
    assertType('int', $own->mine());
    assertType('array<int>|Traversable<mixed, int>', $it);
    assertType('Closure(App\Traversable): void', $cb);
    if ($it instanceof \Traversable) {
        assertType('Traversable<mixed, int>', $it);
    }
}
