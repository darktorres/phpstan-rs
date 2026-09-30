<?php
            /** @param int $x */
            function needsInt($x): void {}
            function f(string $k): void {
                $x = [];
                needsInt($x[$k]);
            }
        