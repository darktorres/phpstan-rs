<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $s): void {
                sink(sprintf('0%s', $s));
            }
        