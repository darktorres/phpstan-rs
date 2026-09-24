<?php
            function needsString(string $s): void {}
            /** @param int[] $ints */
            function f(array $ints): void {
                usort($ints, function ($a, $b) {
                    needsString($a);
                    return $a <=> $b;
                });
            }
        