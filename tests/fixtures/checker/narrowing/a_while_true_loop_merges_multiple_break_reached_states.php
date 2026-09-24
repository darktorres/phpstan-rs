<?php
            function f(bool $flag): int {
                while (true) {
                    if ($flag) {
                        $y = 1;
                        break;
                    }
                    $y = 2;
                    break;
                }
                return $y;
            }
        