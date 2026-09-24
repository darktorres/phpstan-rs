<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (!count($x)) {
                    return;
                }
                sink($x);
            }
        