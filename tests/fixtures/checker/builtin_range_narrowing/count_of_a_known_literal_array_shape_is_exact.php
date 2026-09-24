<?php
            /** @param int<0, 2> $x */
            function needsNarrower(int $x): void {}
            function test(): void {
                needsNarrower(count(['a', 'b', 'c']));
            }
        