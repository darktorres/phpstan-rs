<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $a
             *  @param array<string, mixed> $b
             */
            function f(array $a, array $b, string $val): void {
                $arr = [];
                if (is_int($a['id'])) {
                    $arr[$a['id']] = $val;
                }
                if (is_string($b['id'])) {
                    $arr[$b['id']] = $val;
                }
                assertType('array<(int|string), string>', $arr);
            }
        