<?php
use function PHPStan\Testing\assertType;

/**
 * @param ?array{a: int} $row
 */
function read(?array $row): void {
    $none = null;
    assertType('null', $none['a']);
    assertType('int|null', $row['a']);
    assertType('int', $row['a'] ?? 0);
    if (isset($row['a'])) {
        assertType('int', $row['a']);
    }
    $none['b'] = 1;
    assertType('array{b: 1}', $none);
}

read(['a' => 1]);
read(null);
