<?php
            /** @param string $x */
            function needsString($x): void {}
            function f(array $a, int $b): void {
                needsString(min($a, $b));
            }
        