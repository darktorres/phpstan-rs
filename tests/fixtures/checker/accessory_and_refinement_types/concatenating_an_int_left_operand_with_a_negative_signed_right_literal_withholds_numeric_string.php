<?php
            /** @param numeric-string $s */
            function needs_numeric_string(string $s): void {}
            function f(int $x): void {
                needs_numeric_string($x . '-5');
            }
        