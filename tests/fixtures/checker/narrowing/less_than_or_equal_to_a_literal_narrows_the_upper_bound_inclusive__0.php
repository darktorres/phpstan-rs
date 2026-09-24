<?php
            /** @param int<0, 10> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if ($x <= 10 && $x >= 0) {
                    needsNarrower($x);
                }
            }
        