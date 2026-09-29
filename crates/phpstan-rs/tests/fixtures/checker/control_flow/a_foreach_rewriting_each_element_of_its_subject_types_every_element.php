<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

/** @param list<int> $days */
function calendar(array $days): void
{
    $items = [];
    foreach ($days as $day) {
        $items[$day] = ['nb' => 1];
    }
    foreach ($items as $day => $data) {
        $items[$day]['file'] = 'x.jpg';
    }
    assertType("array<int, array{nb: 1, file: 'x.jpg'}>", $items);
    if ($items !== []) {
        assertType("non-empty-array<int, array{nb: 1, file: 'x.jpg'}>", $items);
    }
}
