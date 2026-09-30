<?php
use function PHPStan\Testing\assertType;

/**
 * @param 'text/html'|'text/plain' $type
 * @param array{a?: int, b?: int} $opt
 */
function f(string $type, array $opt, string $k): void {
    $contents = [];
    $contents[$type] = 'x';
    assertType("non-empty-array{'text/html'?: 'x', 'text/plain'?: 'x'}", $contents);

    $withField = ['n' => 1];
    $withField[$type] = 'x';
    assertType("array{n: 1, 'text/html'?: 'x', 'text/plain'?: 'x'}", $withField);

    if ($opt !== []) {
        $opt[$k] = 5;
        assertType('non-empty-array{a?: int, b?: int, ...<(int|string), 5>}', $opt);
    }
}

f('text/html', [], 'z');
f('text/plain', ['a' => 1], 'b');
