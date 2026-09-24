<?php
            /** @param-out int $x */
            function f(int|string &$x, bool $cond): void {
                if ($cond) {
                    $x = 1;
                } else {
                    $x = 2;
                }
            }
        