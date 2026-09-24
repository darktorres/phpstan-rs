<?php
            /** @param non-empty-list<int> $x */
            function sink(array $x): void {}
            /** @param list<int> $x */
            function f(array $x): void {
                if ($x !== []) {
                    sink($x);
                }
            }
        