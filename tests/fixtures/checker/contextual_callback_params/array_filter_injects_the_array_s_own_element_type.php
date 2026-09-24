<?php
            function needsString(string $s): void {}
            /** @param int[] $ints */
            function f(array $ints): void {
                array_filter($ints, function ($x) {
                    needsString($x);
                    return true;
                });
            }
        