<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            /** @param non-empty-string $x */
            function f($x): void {
                sink($x);
            }
        