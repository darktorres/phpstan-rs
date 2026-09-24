<?php
            /** @param int<1, 10> $x */
            function needsExact(int $x): void {}
            function test(): void {
                needsExact(random_int(1, 10));
            }
        