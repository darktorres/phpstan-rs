<?php
            function f(?string $a, ?string $b): int {
                if (isset($a, $b)) {
                    return strlen($a) + strlen($b);
                }
                return 0;
            }
        