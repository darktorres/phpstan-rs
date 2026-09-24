<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $unknown): void {
                sink($unknown . '/');
            }
        