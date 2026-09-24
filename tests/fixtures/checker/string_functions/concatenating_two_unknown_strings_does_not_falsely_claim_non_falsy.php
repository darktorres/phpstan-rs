<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $a, string $b): void {
                sink($a . $b);
            }
        