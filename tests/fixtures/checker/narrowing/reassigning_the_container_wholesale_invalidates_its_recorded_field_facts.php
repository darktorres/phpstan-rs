<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array<string, array<string, string|int|bool|null>> $rows
             * @param array<string, array<string, string|int|bool|null>> $other
             */
            function f(array $rows, array $other): void {
                foreach ($rows as $type => &$row) {
                    $row['flag_b'] = true;
                }
                unset($row);

                $rows = $other;

                foreach ($rows as $t2 => $prow) {
                    assertType('string|int|bool|null', $prow['flag_b']);
                }
            }
        