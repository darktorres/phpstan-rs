<?php
            /** @param int|false $x */
            function f($x): int {
                if (is_numeric($x)) {
                    return $x;
                }
                return 0;
            }
        