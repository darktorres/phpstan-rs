<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param string[] $keys
             *  @param int[] $vals */
            function f(array $keys, array $vals) {
                $combined = array_combine($keys, $vals);
                needsInts($combined);
                needsStrings($combined);
            }
        