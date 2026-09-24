<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function bail(): never { throw new \Exception(); }
            function f(array $x): void {
                if (1 > count($x)) {
                    bail();
                }
                sink($x);
            }
            function g(array $x): void {
                if (0 >= count($x)) {
                    bail();
                }
                sink($x);
            }
        