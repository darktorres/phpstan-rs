<?php
            /** @param int<0, max> $x */
            function needsNonNegative(int $x): void {}
            function test(array $a): void {
                needsNonNegative(count($a));
                needsNonNegative(sizeof($a));
            }
        