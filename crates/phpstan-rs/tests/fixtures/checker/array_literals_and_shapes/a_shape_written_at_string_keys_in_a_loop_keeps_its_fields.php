<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

/** @param array<string, int> $extra */
function results(array $extra, string $name, int $score): void
{
    $results = ['items' => [1, 2], 'debug' => []];
    $results[$name] = $score;
    assertType('array{items: array{0: 1, 1: 2}|int, debug: array{}|int, ...<(int|string), int>}', $results);

    $page = ['items' => [], 'total' => 0];
    foreach ($extra as $key => $value) {
        if ($key !== 'total') {
            $page[$key] = $value;
        }
    }
    assertType('array{items: array{}|int, total: int, ...<string, int>}', $page);
}
