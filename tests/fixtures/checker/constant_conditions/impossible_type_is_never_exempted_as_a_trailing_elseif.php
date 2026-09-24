<?php
            function f(int $x) {
                if ($x > 5) {
                    echo 1;
                } elseif (is_string($x)) {
                    echo 2;
                }
            }
        