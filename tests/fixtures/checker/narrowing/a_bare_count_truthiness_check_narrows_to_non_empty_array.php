<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (count($x)) {
                    sink($x);
                }
            }
            function g(array $x): void {
                while (count($x)) {
                    sink($x);
                    break;
                }
            }
        