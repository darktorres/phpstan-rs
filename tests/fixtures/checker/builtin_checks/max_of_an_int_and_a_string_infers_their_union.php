<?php
            /** @param int $x */
            function needsInt($x): void {}
            function f(int $a, string $b): void {
                needsInt(max($a, $b));
            }
        