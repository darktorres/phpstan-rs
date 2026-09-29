<?php
final class Foo { public int $x = 1; public function m(): int { return 1; } }

/**
 * @template T of Foo|string
 * @param T $v
 */
function f($v): void
{
    echo $v->x;
    echo $v->m();
}
