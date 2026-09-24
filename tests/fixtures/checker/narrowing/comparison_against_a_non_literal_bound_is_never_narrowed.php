<?php
            /** @param int<0, max> $x */
            function needsNarrower(int $x): void {}
            function test(int $x, int $limit): void {
                if ($x < $limit) {
                    needsNarrower($x);
                }
            }
        