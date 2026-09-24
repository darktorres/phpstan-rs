<?php
            /** @param int<4, 40> $x */
            function needsNarrower(int $x): void {}
            /** @param int<1, 10> $x */
            function test(int $x): void {
                needsNarrower($x << 2);
            }
        