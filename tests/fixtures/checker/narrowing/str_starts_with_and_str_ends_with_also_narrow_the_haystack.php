<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $a, string $b): void {
                if (str_starts_with($a, 'foo')) {
                    sink($a);
                }
                if (str_ends_with($b, 'bar')) {
                    sink($b);
                }
            }
        