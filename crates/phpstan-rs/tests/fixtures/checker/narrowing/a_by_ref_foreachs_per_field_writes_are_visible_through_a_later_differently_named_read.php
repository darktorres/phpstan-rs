<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function f(array $rows): void {
                foreach ($rows as $type => &$row) {
                    if ($row['flag_a'] = ($type === 'x' ? true : false)) {
                        $row['flag_c'] = 1;
                    } else {
                        $row['flag_c'] = 2;
                    }
                    $row['flag_b'] = true;
                }
                unset($row);

                foreach (['a', 'b'] as $t) {
                    $tmp = 1;
                }

                foreach ($rows as $t2 => $prow) {
                    assertType('true|false', $prow['flag_a']);
                    assertType('true', $prow['flag_b']);
                    assertType('1|2', $prow['flag_c']);
                }
            }
        