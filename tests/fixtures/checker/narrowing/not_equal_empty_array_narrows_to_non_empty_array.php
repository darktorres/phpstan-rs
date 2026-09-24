<?php
            /** @param non-empty-array<int> $x */
            function sink(array $x): void {}
            /** @param array<int> $x */
            function f(array $x): void {
                if ($x !== []) {
                    sink($x);
                }
            }
        