<?php
            /** @param array{a: int, ...} $x */
            function f(array $x): void {
                if (isset($x['b'])) {
                    echo 1;
                }
            }
        