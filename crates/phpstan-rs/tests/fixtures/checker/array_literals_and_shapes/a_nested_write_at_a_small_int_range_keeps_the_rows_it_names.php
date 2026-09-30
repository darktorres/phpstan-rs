<?php
use function PHPStan\Testing\assertType;

function sharpen(float $a): void {
    $matrix = [[-1.0, -1.0, -1.0], [-1.0, $a, -1.0], [-1.0, -1.0, -1.0]];
    for ($i = 0; $i < 3; $i++) {
        for ($j = 0; $j < 3; $j++) {
            $matrix[$i][$j] /= 2.0;
        }
    }
    assertType('array{0: array{0: float, 1: float, 2: float}, 1: array{0: float, 1: float, 2: float}, 2: array{0: float, 1: float, 2: float}}', $matrix);
}

sharpen(40.0);
