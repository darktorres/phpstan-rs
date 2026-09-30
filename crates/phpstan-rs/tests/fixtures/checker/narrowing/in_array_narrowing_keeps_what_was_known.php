<?php
use function PHPStan\Testing\assertType;

/**
 * @param non-empty-string $order
 */
function rank(string $order): void {
    if (in_array($order, ['asc', 'desc', ''], true)) {
        assertType("'asc'|'desc'", $order);
    }
}

rank('asc');
rank('x');
