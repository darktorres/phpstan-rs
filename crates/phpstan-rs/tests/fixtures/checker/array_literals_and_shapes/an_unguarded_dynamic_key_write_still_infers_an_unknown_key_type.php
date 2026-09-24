<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $row */
            function f(array $row, string $val): void {
                $arr = [];
                $arr[$row['id']] = $val;
                assertType('array<(int|string), string>', $arr);
            }
        