<?php
/**
 * @param array<string, int> $map
 */
function read(array $map, string $k): int {
    return $map[$k];
}

echo read(['k' => 1], 'k');
