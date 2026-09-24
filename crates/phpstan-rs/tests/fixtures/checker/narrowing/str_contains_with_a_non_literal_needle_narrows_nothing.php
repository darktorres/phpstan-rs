<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x, string $needle): void {
                if (str_contains($x, $needle)) {
                    sink($x);
                }
            }
        