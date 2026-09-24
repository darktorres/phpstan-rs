<?php
            /** @param int<10, max> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if ($x < 10) {
                    return;
                }
                needsNarrower($x);
            }
        