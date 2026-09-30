<?php
use function PHPStan\Testing\assertType;

/**
 * @param list<string> $states
 */
function countStates(array $states): void {
    $counts = [
        'active' => 0,
        'inactive' => 0,
        'missing' => 0,
        'merged' => 0,
    ];
    foreach ($states as $state) {
        $counts[$state]++;
    }
    $counts['missing']++;
    assertType('array{active: int<0, max>, inactive: int<0, max>, missing: int<1, max>, merged: int<0, max>, ...<(int|string), int<1, max>>}', $counts);
}

countStates([]);
countStates(['active', 'active', 'inactive', 'active', 'merged', 'retired', 'retired', 'active']);
