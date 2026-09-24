<?php
            /** @param string $x */
            function needsString($x): void {}
            /** @param list<int> $xs */
            function f(array $xs): void {
                needsString(array_find_key($xs, fn($x, $k) => $x > 0));
            }
        