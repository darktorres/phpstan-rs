<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function bail(): never { throw new \Exception(); }
            function f(array $x): void {
                if (count($x) < 1) {
                    bail();
                }
                sink($x);
            }
            function g(array $x): void {
                if (count($x) <= 0) {
                    bail();
                }
                sink($x);
            }
        