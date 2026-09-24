<?php
            /** @param int<1, 9> $x */
            function needsNarrower(int $x): void {}
            function test(): void {
                needsNarrower(random_int(1, 10));
            }
        