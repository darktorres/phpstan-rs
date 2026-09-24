<?php
            /** @param array{0: int, name: string, nested: array{a: int}} $row */
            function f(array $row): void {
                [$a, $b] = $row;
                ['name' => $n, 'age' => $age] = $row;
                ['nested' => ['a' => $x, 'b' => $y]] = $row;
                [, , $third] = $row;
            }
        