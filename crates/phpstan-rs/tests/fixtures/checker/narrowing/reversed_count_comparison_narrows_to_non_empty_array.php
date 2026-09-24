<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (0 < count($x)) {
                    sink($x);
                }
            }
            function g(array $x): void {
                if (1 <= count($x)) {
                    sink($x);
                }
            }
        