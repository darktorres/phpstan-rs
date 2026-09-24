<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function f(array $rows, string $key): void {
                foreach ($rows as $type => &$row) {
                    $row['flag_b'] = true;
                }
                unset($row);

                $one = $rows[$key];
                assertType('true', $one['flag_b']);
            }
        