<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $s): void {
                sink(sprintf('%s', $s));
            }
        