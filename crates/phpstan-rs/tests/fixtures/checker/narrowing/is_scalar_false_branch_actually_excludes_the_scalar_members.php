<?php
            /** @param int|array $x */
            function f($x): array {
                if (! is_scalar($x)) {
                    return $x;
                }
                return [];
            }
        