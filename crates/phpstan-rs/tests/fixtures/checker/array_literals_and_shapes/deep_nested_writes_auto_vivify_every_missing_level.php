<?php
            function straight(): void {
                $x = [];
                $x['a']['b'][] = 1;
                \PHPStan\Testing\assertType('array{a: array{b: non-empty-list<1>}}', $x);
            }
            function nested(array $items): void {
                $x = [];
                foreach ($items as $i) { foreach ($items as $j) { $x[$i][$j][] = 1; } }
                \PHPStan\Testing\assertType('array<(int|string), array<(int|string), non-empty-list<1>>>', $x);
            }
        