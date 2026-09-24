<?php
            /** @param int<8, max> $x */
            function needsUnbounded(int $x): void {}
            /** @param int<1, max> $x */
            function test(int $x): void {
                needsUnbounded($x << 3);
            }
        