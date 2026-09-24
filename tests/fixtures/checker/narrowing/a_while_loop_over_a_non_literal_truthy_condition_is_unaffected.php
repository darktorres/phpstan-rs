<?php
            function f(bool $cond): int {
                while ($cond) {
                    $y = 1;
                    break;
                }
                return $y;
            }
        