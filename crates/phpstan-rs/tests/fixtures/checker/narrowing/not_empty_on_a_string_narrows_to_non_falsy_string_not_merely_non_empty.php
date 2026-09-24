<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!empty($x)) {
                    sink($x);
                }
            }
        