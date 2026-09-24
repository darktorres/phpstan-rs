<?php
            function f(?string $x): int {
                if (isset($x)) {
                    return strlen($x);
                }
                return 0;
            }
        