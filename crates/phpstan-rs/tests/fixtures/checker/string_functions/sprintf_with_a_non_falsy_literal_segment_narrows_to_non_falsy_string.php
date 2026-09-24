<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(int $n): void {
                sink(sprintf('value: %d', $n));
            }
        