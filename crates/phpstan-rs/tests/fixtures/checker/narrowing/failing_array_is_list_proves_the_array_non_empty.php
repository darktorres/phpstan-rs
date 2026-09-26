<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (array_is_list($x)) {
                } else {
                    sink($x);
                }
            }
        