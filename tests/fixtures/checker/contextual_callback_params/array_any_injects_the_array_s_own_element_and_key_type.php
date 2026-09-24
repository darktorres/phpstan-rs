<?php
            function needsString(string $s): void {}
            /** @param int[] $ints */
            function f(array $ints): bool {
                return array_any($ints, function ($x, $k) {
                    needsString($x);
                    return true;
                });
            }
        