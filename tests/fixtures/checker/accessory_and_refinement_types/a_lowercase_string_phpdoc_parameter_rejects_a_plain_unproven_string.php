<?php
            /** @param lowercase-string $s */
            function needs_lowercase(string $s): void {}
            function f(string $x): void {
                needs_lowercase($x);
            }
        