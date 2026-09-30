<?php
            use function PHPStan\Testing\assertType;
            /** @param array{id: int|string} $row */
            function f(array $row, string $key): void {
                $row[$key] = 5;
                assertType('array{id: int|string, ...<(int|string), 5>}', $row);
            }
        