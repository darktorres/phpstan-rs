<?php
            use function PHPStan\Testing\assertType;
            /** @param array{id: int|string} $row */
            function f(array $row): void {
                $key = 'id';
                $row[$key] = 5;
                assertType('array{id: 5}', $row);
            }
        