<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $format, int $n): void {
                sink(sprintf($format, $n));
            }
        