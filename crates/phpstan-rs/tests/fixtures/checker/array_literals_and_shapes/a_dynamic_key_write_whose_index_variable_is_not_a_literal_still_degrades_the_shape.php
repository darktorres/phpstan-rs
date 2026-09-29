<?php
            use function PHPStan\Testing\assertType;
            /** @param array{id: int|string} $row */
            function f(array $row, string $key): void {
                $row[$key] = 5;
                assertType('non-empty-array<(int|string), int|string>', $row);
            }
        