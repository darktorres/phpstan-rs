<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param int[] $xs */
            function f(array $xs) {
                $chunks = array_chunk($xs, 2);
                needsStrings($chunks);
            }
        