<?php
            /** @param int|string $x */
            function f($x): int|string {
                if (is_numeric($x)) {
                    return $x;
                }
                return 0;
            }
        