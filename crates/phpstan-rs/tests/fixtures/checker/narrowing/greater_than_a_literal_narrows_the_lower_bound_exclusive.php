<?php
            /** @param int<1, max> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if ($x > 0) {
                    needsNarrower($x);
                }
            }
        