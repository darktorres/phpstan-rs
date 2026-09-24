<?php
            /** @param int<0, max> $x */
            function needsNarrower(int|string $x): void {}
            function test(int|string $x): void {
                if ($x < 10) {
                    needsNarrower($x);
                }
            }
        