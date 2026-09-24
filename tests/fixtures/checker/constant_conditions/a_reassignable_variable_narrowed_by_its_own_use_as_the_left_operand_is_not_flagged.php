<?php
            function f(bool $cond) {
                $same = true;
                if ($cond) {
                    $same = false;
                }
                if ($same && true) {
                    echo 1;
                }
            }
        