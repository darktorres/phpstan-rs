<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!is_numeric($x)) {
                    return;
                }
                sink($x);
            }
            function g(string $x): void {
                if (is_numeric($x)) {
                    return;
                }
                sink($x);
            }
        