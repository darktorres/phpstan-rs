<?php
            /** @param int<1, 10> $x */
            function needsExact(int $x): void {}
            function test(): void {
                needsExact(rand(1, 10));
                needsExact(mt_rand(1, 10));
            }
        