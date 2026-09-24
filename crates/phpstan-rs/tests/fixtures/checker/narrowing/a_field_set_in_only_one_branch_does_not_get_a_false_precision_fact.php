<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function f(array $rows): void {
                foreach ($rows as $type => &$row) {
                    if ($type === 'x') {
                        $row['maybe'] = true;
                    }
                }
                unset($row);

                foreach ($rows as $t2 => $prow) {
                    assertType('string|int|bool|null', $prow['maybe']);
                }
            }
        