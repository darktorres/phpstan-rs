<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param int[] $xs */
            function f(array $xs) {
                $filtered = array_filter($xs, fn($x) => $x > 0);
                needsInts($filtered);
                needsStrings($filtered);
            }
        