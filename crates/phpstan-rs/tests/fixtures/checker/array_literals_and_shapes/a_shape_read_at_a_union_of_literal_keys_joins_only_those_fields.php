<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

/** @param array{sql: string, dql: string, labels: array<int|string, string>|null} $level */
function column(array $level, bool $forDql): string
{
    $key = $forDql ? 'dql' : 'sql';
    assertType('string', $level[$key]);
    [$key => $field] = $level;
    assertType('string', $field);

    return ' AND ' . $level[$key];
}
