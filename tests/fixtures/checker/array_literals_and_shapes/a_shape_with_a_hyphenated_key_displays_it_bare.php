<?php
            function needsInt(int $x): void {}
            /** @param array{'content-type': string} $x */
            function f(array $x): void {
                needsInt($x);
            }
        