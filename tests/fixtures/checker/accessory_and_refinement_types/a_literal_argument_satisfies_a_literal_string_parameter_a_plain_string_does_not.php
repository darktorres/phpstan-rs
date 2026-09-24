<?php
            /** @param literal-string $x */
            function sink($x): void {}
            function f(): void {
                sink("hello");
            }
            function g(string $x): void {
                sink($x);
            }
        