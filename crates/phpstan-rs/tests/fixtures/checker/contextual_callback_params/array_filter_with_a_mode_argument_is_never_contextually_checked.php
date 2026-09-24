<?php
            /** @param int[] $ints */
            function f(array $ints): void {
                array_filter($ints, function ($x) {
                    return true;
                }, ARRAY_FILTER_USE_KEY);
            }
        