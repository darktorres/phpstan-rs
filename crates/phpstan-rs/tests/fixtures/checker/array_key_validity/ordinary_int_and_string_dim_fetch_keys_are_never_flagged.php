<?php
            /** @param int[] $arr */
            function test(array $arr, int $i, string $s) {
                return [$arr[$i], $arr[$s], $arr[3], $arr['k']];
            }
        