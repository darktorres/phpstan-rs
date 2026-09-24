<?php
            /** @param 4 $x */
            function needsFour(int $x): void {}
            function test(): void {
                needsFour(abs(-5));
            }
        