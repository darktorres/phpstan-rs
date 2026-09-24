<?php
            /** @param int<0, max> $x */
            function needsNonNegative(int $x): void {}
            function test(string $s): void {
                needsNonNegative(strlen($s));
            }
        