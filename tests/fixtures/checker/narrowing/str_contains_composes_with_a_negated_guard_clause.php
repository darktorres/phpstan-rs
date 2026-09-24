<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!str_contains($x, 'foo')) {
                    return;
                }
                sink($x);
            }
        