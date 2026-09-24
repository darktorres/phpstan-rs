<?php
            /** @param string $x */
            function needsString($x): void {}
            function f(int $a, int $b): void {
                needsString(min($a, $b));
            }
        