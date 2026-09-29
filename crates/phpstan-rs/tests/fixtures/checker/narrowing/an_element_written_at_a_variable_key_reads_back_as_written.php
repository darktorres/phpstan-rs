<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

/** @param array<int, array{nb: int}> $items */
function annotate(array $items, int $day, bool $withWeekday): void
{
    $items[$day]['file'] = 'x.jpg';
    assertType("array{nb: int, file: 'x.jpg'}", $items[$day]);
    if ($withWeekday) {
        $items[$day]['dow'] = 1;
        assertType("array{nb: int, file: 'x.jpg', dow: 1}", $items[$day]);
    }
    assertType("array{nb: int, file: 'x.jpg', dow?: 1}", $items[$day]);
}
