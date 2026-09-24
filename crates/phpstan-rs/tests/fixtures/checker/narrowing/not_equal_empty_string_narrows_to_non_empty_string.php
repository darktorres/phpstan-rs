<?php
            function needsNonEmpty(): void {}
            /** @param non-empty-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if ($x !== '') {
                    sink($x);
                }
            }
        