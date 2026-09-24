<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function f(array $rows): void {
                foreach ($rows as $type => &$row) {
                    $row['flag_b'] = true;
                }
                unset($row);

                mutateSomehow($rows);

                foreach ($rows as $t2 => $prow) {
                    assertType('string|int|bool|null', $prow['flag_b']);
                }
            }

            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function mutateSomehow(array &$rows): void {}
        