<?php
            /** @param int[] $ints */
            function f(array $ints): void {
                usort($ints, function ($a, $b) {
                    return $a <=> $b;
                });
            }
        