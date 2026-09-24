<?php
            function f(bool $x) {
                if ($x) {
                    echo 1;
                } elseif (true) {
                    echo 2;
                } elseif ($x) {
                    echo 3;
                }
            }
        