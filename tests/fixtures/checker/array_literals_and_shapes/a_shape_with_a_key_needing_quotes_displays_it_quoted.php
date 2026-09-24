<?php
            function needsInt(int $x): void {}
            /** @param array{'foo bar': string} $x */
            function f(array $x): void {
                needsInt($x);
            }
        