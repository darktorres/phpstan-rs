<?php
            /** @param int|array<int> $x */
            function f($x): int {
                if (! is_countable($x)) {
                    return $x;
                }
                return count($x);
            }
        