<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $row */
            function f(array $row, string $val): void {
                $arr = [];
                if (is_int($row['id'])) {
                    $arr[$row['id']] = $val;
                    assertType('non-empty-array<int, string>', $arr);
                }
            }
        