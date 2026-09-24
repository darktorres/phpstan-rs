<?php
            /** @param-out int $x */
            function f(int|string &$x): void {
                $unrelated = 1;
            }
        