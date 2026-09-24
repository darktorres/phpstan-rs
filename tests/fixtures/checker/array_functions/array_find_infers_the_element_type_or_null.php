<?php
            /** @param string $x */
            function needsString($x): void {}
            /** @param int[] $xs */
            function f(array $xs): void {
                needsString(array_find($xs, fn($x) => $x > 0));
            }
        