<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            function f(array $keys) {
                $a = array_fill(0, 5, 42);
                $b = array_fill_keys($keys, 42);
                needsStrings($a);
                needsStrings($b);
            }
        