<?php
            /** @param-out int $x */
            function f(int|string &$x): void {
                $x = 1;
            }
        