<?php
            /** @param int<1, max>|float $x */
            function needsNumber(int|float $x): void {}
            function test(): void {
                needsNumber(hrtime(true));
            }
        