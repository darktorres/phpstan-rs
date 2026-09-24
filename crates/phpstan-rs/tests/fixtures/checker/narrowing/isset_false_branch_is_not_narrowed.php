<?php
            function f(?string $x): int {
                if (isset($x)) {
                    return 0;
                }
                return strlen($x);
            }
        