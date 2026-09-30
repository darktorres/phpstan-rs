<?php
use function PHPStan\Testing\assertType;

/**
 * @param array{a?: int} $row
 * @param array<string, int> $map
 */
function read(array $row, array $map, string $k): void {
    assertType('int|null', $row['a']);
    if (isset($row['a'])) {
        assertType('int', $row['a']);
    }
    assertType('int', $row['a'] ?? 0);
    if (array_key_exists('a', $row)) {
        assertType('int', $row['a']);
    }
    assertType('int', $map[$k]);
}

/**
 * @param array{0: string}|array{0: string, 1: string} $oneOrTwo
 */
function destructure(array $oneOrTwo): void {
    [$x, $y] = $oneOrTwo;
    assertType('string', $x);
}

read([], ['k' => 1], 'k');
read(['a' => 1], ['k' => 1], 'k');
destructure(['a']);
destructure(['a', 'b']);
