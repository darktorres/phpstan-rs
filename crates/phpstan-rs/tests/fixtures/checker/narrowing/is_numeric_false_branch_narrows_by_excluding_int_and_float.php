<?php
            /** @param int|string $x */
            function f($x): string {
                if (! is_numeric($x)) {
                    return $x;
                }
                return '';
            }
        