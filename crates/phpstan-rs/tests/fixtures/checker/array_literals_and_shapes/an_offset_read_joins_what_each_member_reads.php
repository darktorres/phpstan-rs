<?php
use function PHPStan\Testing\assertType;

function matches(string $s): void {
    preg_match('/^(a)(b)?(c)$/', $s, $m);
    assertType("'b'|''|null", $m[2] ?? null);
    if (isset($m[1])) {
        assertType("'a'", $m[1]);
    }
}

/**
 * @param list<array{crop: float}|array{}> $variants
 */
function crops(array $variants): void {
    foreach ($variants as $v) {
        assertType('float|0', $v['crop'] ?? 0);
    }
}

/**
 * @param ?array{a: int} $row
 */
function nullable(?array $row): void {
    assertType('int', $row['a'] ?? 0);
}

matches('abc');
matches('ac');
matches('x');
crops([['crop' => 1.5], []]);
nullable(['a' => 3]);
nullable(null);
