<?php
            function f(int $base) {
                $fn = function (int $x) use ($base) {
                    return $base + $x;
                };
                return $fn(5);
            }
        