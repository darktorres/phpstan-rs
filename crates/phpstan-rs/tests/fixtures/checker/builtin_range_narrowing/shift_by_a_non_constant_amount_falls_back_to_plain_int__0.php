<?php
            /** @param int<1, 10> $x */
            function needsNarrower(int $x): void {}
            /** @param int<0, 5> $shift */
            function test(int $x, int $shift): void {
                needsNarrower($x << $shift);
            }
        