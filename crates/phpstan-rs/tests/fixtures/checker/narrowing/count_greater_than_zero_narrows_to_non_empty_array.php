<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (count($x) > 0) {
                    sink($x);
                }
            }
            function g(array $x): void {
                if (count($x) >= 1) {
                    sink($x);
                }
            }
        