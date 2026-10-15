<?php
use function PHPStan\Testing\assertType;
/** @template T */
class Box {
    /** @return T */
    public function get(): mixed { return null; }
}
function f(Box $b): void {
    assertType('mixed', $b->get());
}
