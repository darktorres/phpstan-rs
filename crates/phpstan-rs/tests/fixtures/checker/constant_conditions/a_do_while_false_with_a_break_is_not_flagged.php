<?php
            function f(bool $x) {
                do {
                    if ($x) {
                        break;
                    }
                    echo 1;
                } while (false);
            }
        