<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            /** @param non-empty-string $x */
            function f($x): void {
                if (!is_numeric($x)) {
                    throw new \RuntimeException('not numeric');
                }
                sink($x);
            }
        