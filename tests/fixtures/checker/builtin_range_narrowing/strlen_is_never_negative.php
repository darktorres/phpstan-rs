<?php
            /** @param int<0, 5> $x */
            function needsNarrow(int $x): void {}
            function test(string $s): void {
                needsNarrow(strlen($s));
            }
        