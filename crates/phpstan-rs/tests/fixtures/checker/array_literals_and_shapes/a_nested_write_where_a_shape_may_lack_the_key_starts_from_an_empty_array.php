<?php
use function PHPStan\Testing\assertType;

/**
 * @param 'A'|'B' $name
 * @param array{A?: array{counter: int}} $known
 */
function write(string $name, array $known): void {
    $known[$name]['last'] = 'y';
    assertType("non-empty-array{A?: non-empty-array{counter?: int, last?: 'y'}, B?: array{counter?: int, last: 'y'}}", $known);

    $m = ['A' => null];
    $m['A']['k'] = 1;
    assertType('array{A: array{k: 1}}', $m);
}

write('B', ['A' => ['counter' => 1]]);
write('A', []);
