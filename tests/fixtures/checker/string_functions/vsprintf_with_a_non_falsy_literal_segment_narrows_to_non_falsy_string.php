<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(array $args): void {
                sink(vsprintf('value: %d', $args));
            }
        