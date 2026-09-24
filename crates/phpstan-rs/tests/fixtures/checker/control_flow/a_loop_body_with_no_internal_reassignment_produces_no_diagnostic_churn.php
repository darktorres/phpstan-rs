<?php
            function f(int $n): int {
                $total = 0;
                while ($n > 0) {
                    $total = $total + $n;
                    $n = $n - 1;
                }
                return $total;
            }
        